# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A column's header in a history of commits.
git_column_author = Author

# A column's header in a history of commits: its summary.
git_column_commit = Commit

# A column's header in a history of commits.
git_column_date = Date

# A column's header in a history of commits: its short id.
git_column_hash = Hash

# A column's header in a history of commits: the names at each commit.
git_column_names = Branches and tags

# The value beside Commits in a history's Properties: how many commits it lists, all of them.
git_commits = { $count }

# The value beside Commits in a history's Properties when older commits are left out: how many it lists.
git_commits_shown = { $count ->
        [one] { $count } commit shown
       *[other] { $count } commits shown
    }

# A history's detail: the branch checked out.
git_detail_branch = Branch

# A history's detail: how many commits it lists.
git_detail_commits = Commits

git_history = Git history

# The caption of a history of commits; branch: the branch checked out, as Git names it.
git_history_on = Git history · { $branch }

# Under a history of commits.
git_names_cut = Some commits have more branches and tags than are shown

# Under a history of commits: older commits are left, read by Full preview.
git_newest = { $count ->
        [1] Only the newest commit
       *[other] The newest { $count } commits
    }

# Under a history of commits: older commits are left, and some commits' names are left out.
git_newest_names_cut = { $count ->
        [1] Only the newest commit. Some commits have more branches and tags than are shown
       *[other] The newest { $count } commits. Some commits have more branches and tags than are shown
    }

git_no_commit = No commit yet

# A file's or a folder's history of commits, empty.
git_no_commit_changed = No commit changed it

git_not_in_repository = Not in a Git repository

git_plugin_description = A folder's and a file's history of commits in a Git repository, the graph of its merges and what each commit changed, and the files' state marked in the views

git_plugin_title = Git

# What stops working without the Git helper program, on the Plugins page.
git_requirement = Histories of commits

git_states_setting = Show the files' Git state

git_states_setting_description = Mark files changed, staged, new or in conflict, and the folders that hold them, in folders kept in a Git repository.

# What a setting's description says after its first sentence, in its tooltip.
git_states_setting_description_more = The list's Git column says the same.

# Under a history of commits: reading was stopped before the oldest commit.
git_stopped = { $count ->
        [1] Only the newest commit: reading stopped before the oldest
       *[other] The newest { $count } commits: reading stopped before the oldest
    }

# Under a history of commits: reading was stopped, and some commits' names are left out.
git_stopped_names_cut = { $count ->
        [1] Only the newest commit: reading stopped before the oldest. Some commits have more branches and tags than are shown
       *[other] The newest { $count } commits: reading stopped before the oldest. Some commits have more branches and tags than are shown
    }

# A button of the preview's bar: shows the file's or the folder's history of commits.
git_way = History

git_way_tip = Its history of commits, read from Git
