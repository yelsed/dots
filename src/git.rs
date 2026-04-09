use anyhow::{Context, Result};
use git2::{Cred, FetchOptions, PushOptions, RemoteCallbacks, Repository, Signature};
use std::cell::Cell;
use std::path::Path;

/// Open the git repository at the given path
pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::open(path).with_context(|| format!("Failed to open git repo at {}", path.display()))
}

/// Clone a remote repository
pub fn clone_repo(url: &str, path: &Path) -> Result<Repository> {
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(auth_callbacks());

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);

    builder
        .clone(url, path)
        .with_context(|| format!("Failed to clone {} to {}", url, path.display()))
}

/// Stage all changes and commit
pub fn commit_all(repo: &Repository, message: &str) -> Result<git2::Oid> {
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = get_signature(repo)?;

    let parent = match repo.head() {
        Ok(head) => {
            let commit = head.peel_to_commit()?;
            Some(commit)
        }
        Err(_) => None, // Initial commit
    };

    let parents: Vec<&git2::Commit> = parent.iter().collect();

    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .context("Failed to create commit")?;

    Ok(oid)
}

/// Push to remote
pub fn push(repo: &Repository, remote_name: &str) -> Result<()> {
    let mut remote = repo
        .find_remote(remote_name)
        .with_context(|| format!("Remote '{}' not found", remote_name))?;

    let head = repo.head()?;
    let branch = head
        .shorthand()
        .unwrap_or("main");
    let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);

    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(auth_callbacks());

    remote
        .push(&[&refspec], Some(&mut push_opts))
        .context("Failed to push to remote")?;

    Ok(())
}

/// Fetch from remote and check if remote is ahead
pub fn fetch_and_check(repo: &Repository, remote_name: &str) -> Result<RemoteStatus> {
    let mut remote = repo.find_remote(remote_name)?;

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(auth_callbacks());

    remote.fetch(&[] as &[&str], Some(&mut fetch_opts), None)?;

    let head = repo.head()?;
    let local_oid = head.target().context("No local HEAD")?;

    let branch_name = head.shorthand().unwrap_or("main");
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);

    let remote_oid = match repo.find_reference(&remote_ref) {
        Ok(r) => r.target().context("No remote HEAD target")?,
        Err(_) => return Ok(RemoteStatus::UpToDate),
    };

    if local_oid == remote_oid {
        Ok(RemoteStatus::UpToDate)
    } else {
        // Check if remote is ahead
        let (ahead, behind) = repo.graph_ahead_behind(local_oid, remote_oid)?;
        if behind > 0 {
            Ok(RemoteStatus::Behind(behind))
        } else if ahead > 0 {
            Ok(RemoteStatus::Ahead(ahead))
        } else {
            Ok(RemoteStatus::UpToDate)
        }
    }
}

/// Pull (fast-forward) from remote
pub fn pull(repo: &Repository, remote_name: &str) -> Result<()> {
    let mut remote = repo.find_remote(remote_name)?;

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(auth_callbacks());

    remote.fetch(&[] as &[&str], Some(&mut fetch_opts), None)?;

    let head = repo.head()?;
    let branch_name = head.shorthand().unwrap_or("main");
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);

    let remote_reference = repo.find_reference(&remote_ref)?;
    let remote_oid = remote_reference.target().context("No remote target")?;
    let remote_commit = repo.find_annotated_commit(remote_oid)?;

    let (analysis, _) = repo.merge_analysis(&[&remote_commit])?;

    if analysis.is_up_to_date() {
        return Ok(());
    }

    if analysis.is_fast_forward() {
        let mut reference = repo.find_reference(&format!("refs/heads/{}", branch_name))?;
        reference.set_target(remote_oid, "dots pull: fast-forward")?;
        repo.set_head(&format!("refs/heads/{}", branch_name))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    } else {
        anyhow::bail!("Cannot fast-forward. Please resolve conflicts manually with git.");
    }

    Ok(())
}

/// Walk history from HEAD and return the commit time (unix seconds) of the
/// most recent commit that touched `path` (which may be a file or directory,
/// specified as a repo-relative path).
pub fn last_commit_time_for_path(repo: &Repository, path: &str) -> Result<Option<i64>> {
    let target = std::path::Path::new(path);

    let mut walk = repo.revwalk()?;
    walk.push_head()?;
    walk.set_sorting(git2::Sort::TIME)?;

    for oid_result in walk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;

        let parent_tree = if commit.parent_count() == 0 {
            None
        } else {
            Some(commit.parent(0)?.tree()?)
        };

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

        // Walk deltas via the iterator API — the `foreach` callback treats
        // returning `false` as a libgit2 error, which would abort the whole
        // function. Iterating directly lets us break cleanly on a match.
        let mut touched = false;
        for delta in diff.deltas() {
            for file in [delta.old_file().path(), delta.new_file().path()] {
                if let Some(p) = file {
                    if p == target || p.starts_with(target) {
                        touched = true;
                        break;
                    }
                }
            }
            if touched {
                break;
            }
        }

        if touched {
            return Ok(Some(commit.time().seconds()));
        }
    }

    Ok(None)
}

/// Commit time (unix seconds) of the remote tracking branch's HEAD.
/// Assumes a prior `fetch_and_check` has populated the remote ref.
pub fn remote_head_time(repo: &Repository, remote_name: &str) -> Result<i64> {
    let head = repo.head()?;
    let branch_name = head.shorthand().unwrap_or("main");
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);
    let reference = repo.find_reference(&remote_ref)?;
    let oid = reference.target().context("No remote ref target")?;
    let commit = repo.find_commit(oid)?;
    Ok(commit.time().seconds())
}

/// Get list of changed files between local HEAD and remote HEAD
pub fn changed_files(repo: &Repository, remote_name: &str) -> Result<Vec<String>> {
    let head = repo.head()?;
    let local_oid = head.target().context("No local HEAD")?;
    let local_commit = repo.find_commit(local_oid)?;
    let local_tree = local_commit.tree()?;

    let branch_name = head.shorthand().unwrap_or("main");
    let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);
    let remote_reference = repo.find_reference(&remote_ref)?;
    let remote_oid = remote_reference.target().context("No remote target")?;
    let remote_commit = repo.find_commit(remote_oid)?;
    let remote_tree = remote_commit.tree()?;

    let diff = repo.diff_tree_to_tree(Some(&local_tree), Some(&remote_tree), None)?;

    let mut files = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path() {
                files.push(path.display().to_string());
            }
            true
        },
        None,
        None,
        None,
    )?;

    Ok(files)
}

#[derive(Debug)]
pub enum RemoteStatus {
    UpToDate,
    Behind(usize),
    Ahead(usize),
}

fn get_signature(repo: &Repository) -> Result<Signature<'static>> {
    // Try repo config first, fall back to defaults
    match repo.signature() {
        Ok(sig) => Ok(Signature::now(
            sig.name().unwrap_or("dots"),
            sig.email().unwrap_or("dots@localhost"),
        )?),
        Err(_) => Ok(Signature::now("dots", "dots@localhost")?),
    }
}

fn auth_callbacks<'a>() -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    let attempts = Cell::new(0usize);
    callbacks.credentials(move |url, username, allowed| {
        let n = attempts.get();
        if n >= 3 {
            return Err(git2::Error::from_str(
                "SSH authentication failed. Ensure your SSH key is loaded: ssh-add ~/.ssh/id_ed25519",
            ));
        }
        attempts.set(n + 1);
        credential_callback(url, username, allowed, n)
    });
    callbacks
}

fn credential_callback(
    _url: &str,
    username_from_url: Option<&str>,
    allowed_types: git2::CredentialType,
    attempt: usize,
) -> Result<Cred, git2::Error> {
    if allowed_types.contains(git2::CredentialType::SSH_KEY) {
        let user = username_from_url.unwrap_or("git");
        // First attempt: try SSH agent. Subsequent attempts: fall back to
        // on-disk keys, since the agent may be reachable but empty (or may
        // have keys that the server rejects).
        if attempt == 0 {
            if let Ok(cred) = Cred::ssh_key_from_agent(user) {
                return Ok(cred);
            }
        }
        let home = dirs::home_dir().unwrap_or_default();
        let key = home.join(".ssh/id_ed25519");
        if key.exists() {
            return Cred::ssh_key(user, None, &key, None);
        }
        let key = home.join(".ssh/id_rsa");
        if key.exists() {
            return Cred::ssh_key(user, None, &key, None);
        }
    }
    if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
        // Try git credential helper via environment
        if let (Ok(user), Ok(pass)) = (
            std::env::var("GIT_USERNAME"),
            std::env::var("GIT_PASSWORD"),
        ) {
            return Cred::userpass_plaintext(&user, &pass);
        }
    }
    if allowed_types.contains(git2::CredentialType::DEFAULT) {
        return Cred::default();
    }
    Err(git2::Error::from_str("no authentication available"))
}
