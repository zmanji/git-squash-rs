extern crate clap;
extern crate git2;

use std::error;
use std::fmt;

use clap::{crate_version, App, Arg};

#[derive(Debug)]
enum GitSquashError {
    Git2(git2::Error),
    DirtyRepo,
    SymbolicRef(String),
}

impl fmt::Display for GitSquashError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            GitSquashError::Git2(ref e) => e.fmt(f),
            GitSquashError::DirtyRepo => {
                write!(f, "The repo is dirty, please stash or commit changes")
            }
            GitSquashError::SymbolicRef(ref r) => {
                write!(f, "{} is a symbolic reference cannot be used for squash", r)
            }
        }
    }
}

impl error::Error for GitSquashError {
    fn description(&self) -> &str {
        match *self {
            GitSquashError::Git2(ref e) => e.description(),
            GitSquashError::DirtyRepo => "dirty repo cannot be squashed",
            GitSquashError::SymbolicRef(ref _s) => "symbolic ref cannot be resolved",
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            GitSquashError::Git2(ref e) => Some(e),
            GitSquashError::DirtyRepo => None,
            GitSquashError::SymbolicRef(ref _s) => None,
        }
    }
}

impl From<git2::Error> for GitSquashError {
    fn from(err: git2::Error) -> GitSquashError {
        GitSquashError::Git2(err)
    }
}

// Check if the index or working copy have changes
fn is_dirty(statuses: &git2::Statuses) -> bool {
    if statuses.is_empty() {
        return false;
    }

    let mut dirty_statuses = git2::Status::empty();
    dirty_statuses.insert(git2::Status::INDEX_NEW);
    dirty_statuses.insert(git2::Status::INDEX_MODIFIED);
    dirty_statuses.insert(git2::Status::INDEX_DELETED);
    dirty_statuses.insert(git2::Status::INDEX_RENAMED);
    dirty_statuses.insert(git2::Status::INDEX_TYPECHANGE);
    dirty_statuses.insert(git2::Status::WT_NEW);
    dirty_statuses.insert(git2::Status::WT_MODIFIED);
    dirty_statuses.insert(git2::Status::WT_DELETED);
    dirty_statuses.insert(git2::Status::WT_TYPECHANGE);
    dirty_statuses.insert(git2::Status::WT_RENAMED);
    dirty_statuses.insert(git2::Status::CONFLICTED);

    for s in statuses.iter() {
        if s.status().intersects(dirty_statuses) {
            return true;
        }
    }

    return false;
}

fn squash(branch_name: &str) -> Result<(), GitSquashError> {
    let repo = git2::Repository::discover(".")?;

    // Check if the index or working copy have changes
    let statuses = repo.statuses(None)?;
    let is_dirt = is_dirty(&statuses);

    if is_dirt {
        return Err(GitSquashError::DirtyRepo);
    }

    let head = repo.refname_to_id("HEAD")?;

    let branch = repo
        .find_branch(branch_name, git2::BranchType::Local)?
        .into_reference();
    let branch = branch
        .target()
        .ok_or(GitSquashError::SymbolicRef(branch_name.to_string()))?;

    let mb = repo.merge_base(branch, head)?;

    let mut revwalk = repo.revwalk()?;
    revwalk.push(head)?;
    revwalk.push(mb)?;
    revwalk.hide(branch)?;
    let mut sort = git2::Sort::empty();
    sort.insert(git2::Sort::TIME);
    revwalk.set_sorting(sort);

    let commits_to_squash: Result<Vec<git2::Oid>, git2::Error> = revwalk.collect();
    let commits_to_squash = commits_to_squash?;

    if commits_to_squash.is_empty() {
        println!("No commits to squash");
        return Ok(());
    } else if commits_to_squash.len() == 1 {
        println!("Only one commit to squash.");
        return Ok(());
    }

    let mb_commit = repo.find_commit(mb)?;
    // Soft reset to the merge base
    repo.reset(mb_commit.as_object(), git2::ResetType::Soft, None)?;

    // Write the index to a tree
    let tree_oid = repo.index()?.write_tree()?;

    // The commit on this branch on top of the merge base which
    // we can reuse for the commit message.
    let last_commit = repo.find_commit(*commits_to_squash.last().unwrap())?;

    // Create the commit
    let sig = repo.signature()?;
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        last_commit.message().unwrap(),
        &repo.find_tree(tree_oid)?,
        &[&mb_commit],
    )?;

    Ok(())
}

fn main() {
    let app = App::new("git-squash")
        .version(crate_version!())
        .about("Utility to squash all commits on a branch relative to another branch")
        .arg(
            Arg::with_name("branch")
                .required(true)
                .help("The upstream branch to squash commits of the current branch on to.")
                .index(1)
                .default_value("master"),
        );

    let matches = app.get_matches();

    let branch = matches.value_of("branch").unwrap();

    match squash(branch) {
        Ok(()) => {}
        Err(e) => println!("error: {}", e),
    }
}
