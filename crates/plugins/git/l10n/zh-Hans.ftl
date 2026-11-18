# Simplified Chinese catalog of skwir-plugin-git, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
git_column_author = 作者

# draft
git_column_commit = 提交

# draft
git_column_date = 日期

# draft
git_column_hash = 哈希

# draft
git_column_names = 分支和标签

# draft
git_commits = { $count }

# draft
git_commits_shown = 显示了 { $count } 个提交

# draft
git_detail_branch = 分支

# draft
git_detail_commits = 提交数

# draft
git_history = Git 历史

# draft
git_history_on = Git 历史 · { $branch }

# draft
git_names_cut = 部分提交的分支和标签多于所显示的

# draft
git_newest = { $count ->
        [1] 仅最新的提交
       *[other] 最新的 { $count } 个提交
    }

# draft
git_newest_names_cut = { $count ->
        [1] 仅最新的提交。部分提交的分支和标签多于所显示的
       *[other] 最新的 { $count } 个提交。部分提交的分支和标签多于所显示的
    }

# draft
git_no_commit = 尚无提交

# draft
git_no_commit_changed = 没有提交更改过它

# draft
git_not_in_repository = 不在 Git 仓库中

# draft
git_plugin_description = Git 仓库中文件夹和文件的提交历史、合并关系图、每个提交的更改内容，并在视图中标记文件的状态

# draft
git_plugin_title = Git

# draft
git_requirement = 提交历史

# draft
git_states_setting = 显示文件的 Git 状态

# draft
git_states_setting_description = 在 Git 仓库中的文件夹里，标记已更改、已暂存、新增或有冲突的文件，以及包含它们的文件夹。

# draft
git_states_setting_description_more = 列表视图的 Git 列显示相同的信息。

# draft
git_stopped = { $count ->
        [1] 仅最新的提交：在读到最早的提交之前已停止读取
       *[other] 最新的 { $count } 个提交：在读到最早的提交之前已停止读取
    }

# draft
git_stopped_names_cut = { $count ->
        [1] 仅最新的提交：在读到最早的提交之前已停止读取。部分提交的分支和标签多于所显示的
       *[other] 最新的 { $count } 个提交：在读到最早的提交之前已停止读取。部分提交的分支和标签多于所显示的
    }

# draft
git_way = 历史

# draft
git_way_tip = 其提交历史，从 Git 读取
