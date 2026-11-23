# Simplified Chinese catalog of skwir-core, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
access_another_file = { $name }现在是另一个文件

# draft
access_both_already = { $inside ->
        [yes] { $name }及其中所有内容的访问权限和所属组已是所要求的设置
       *[no] { $name }的访问权限和所属组已是所要求的设置
    }

# draft
access_change_both = { $inside ->
        [yes] 更改{ $name }及其中所有内容的访问权限和所属组
       *[no] 更改{ $name }的访问权限和所属组
    }

# draft
access_change_group = { $inside ->
        [yes] 更改{ $name }及其中所有内容的所属组
       *[no] 更改{ $name }的所属组
    }

# draft
access_change_mode = { $inside ->
        [yes] 更改{ $name }及其中所有内容的使用权限（{ $mode }）
       *[no] 更改{ $name }的使用权限（{ $mode }）
    }

# draft
access_changed_both = { $inside ->
        [yes] 已更改{ $name }及其中所有内容的访问权限和所属组
       *[no] 已更改{ $name }的访问权限和所属组
    }

# draft
access_changed_group = { $inside ->
        [yes] 已更改{ $name }及其中所有内容的所属组
       *[no] 已更改{ $name }的所属组
    }

# draft
access_changed_mode = { $inside ->
        [yes] 已更改{ $name }及其中所有内容的使用权限
       *[no] 已更改{ $name }的使用权限
    }

# draft
access_clause_also = { $who ->
        [u] 所有者
        [g] 所属组
        [o] 其他用户
        [ug] 所有者和所属组
        [uo] 所有者和其他用户
        [go] 所属组和其他用户
        [umask] 所有人（umask { $umask } 屏蔽的权限除外）
       *[all] 所有人
    }还可以{ $does }

# draft
access_clause_no_longer = { $who ->
        [u] 所有者
        [g] 所属组
        [o] 其他用户
        [ug] 所有者和所属组
        [uo] 所有者和其他用户
        [go] 所属组和其他用户
        [umask] 所有人（umask { $umask } 屏蔽的权限除外）
       *[all] 所有人
    }不能再{ $does }

# draft
access_clause_none = { $who ->
        [u] 所有者
        [g] 所属组
        [o] 其他用户
        [ug] 所有者和所属组
        [uo] 所有者和其他用户
        [go] 所属组和其他用户
        [umask] 所有人（umask { $umask } 屏蔽的权限除外）
       *[all] 所有人
    }无权使用它

# draft
access_clause_nothing = { $who ->
        [u] 所有者
        [g] 所属组
        [o] 其他用户
        [ug] 所有者和所属组
        [uo] 所有者和其他用户
        [go] 所属组和其他用户
        [umask] 所有人（umask { $umask } 屏蔽的权限除外）
       *[all] 所有人
    }的权限没有变化

# draft
access_clause_only = { $who ->
        [u] 所有者
        [g] 所属组
        [o] 其他用户
        [ug] 所有者和所属组
        [uo] 所有者和其他用户
        [go] 所属组和其他用户
        [umask] 所有人（umask { $umask } 屏蔽的权限除外）
       *[all] 所有人
    }只能{ $does }

# draft
access_does_both = { $letters ->
        [r] 读取
        [w] 写入
        [x] 运行它
        [rw] 读取和写入
        [rx] 读取和运行它
        [wx] 写入和运行它
        [X] 打开它（对文件而言，仅当已有人可运行它时才可运行）
        [rX] 读取和打开它（对文件而言，仅当已有人可运行它时才可运行）
        [wX] 写入和打开它（对文件而言，仅当已有人可运行它时才可运行）
        [rwX] 读取、写入和打开它（对文件而言，仅当已有人可运行它时才可运行）
       *[rwx] 读取、写入和运行它
    }

# draft
access_does_copy = 做{ $class ->
        [u] 所有者
        [g] 所属组
       *[o] 其他用户
    }能做的事

# draft
access_does_file = { $letters ->
        [r] 读取
        [w] 写入
        [x] 运行它
        [rw] 读取和写入
        [rx] 读取和运行它
        [wx] 写入和运行它
        [X] 在已有人可运行它时运行它
        [rX] 读取，并在已有人可运行它时运行它
        [wX] 写入，并在已有人可运行它时运行它
        [rwX] 读取、写入，并在已有人可运行它时运行它
       *[rwx] 读取、写入和运行它
    }

# draft
access_does_folder = { $letters ->
        [r] 列出其内容
        [w] 更改其内容
        [x] 打开它
        [rw] 列出并更改其内容
        [rx] 列出其内容并打开它
        [wx] 更改其内容并打开它
       *[rwx] 列出其内容、更改其内容并打开它
    }

# draft
access_drive_read_only = 驱动器为只读

# draft
access_each_class = 所有者{ $owner ->
        [none] 无权使用它
       *[some] 可以{ $owner_does }
    }，所属组{ $group ->
        [none] 无权使用它
       *[some] 可以{ $group_does }
    }，其他用户{ $others ->
        [none] 无权使用它
       *[some] 可以{ $others_does }
    }

# draft
access_everyone_may = 所有人{ $all ->
        [none] 都无权使用它
       *[some] 都可以{ $all_does }
    }

# draft
access_failed_not_put_back = { $error }，且{ $not_put_back }

# draft
access_files_and_folders = 文件：{ $files }；文件夹：{ $folders }

# draft
access_folder_keeps_set_id = { $said }；文件夹会保留其已有的 set-ID 位（{ $clears } 可清除这些位）

# draft
access_gone = { $name }已不存在

# draft
access_group_already = { $inside ->
        [yes] { $name }及其中所有内容的所属组已是所要求的设置
       *[no] { $name }的所属组已是所要求的设置
    }

# draft
access_in_trash = { $name }在回收站中：请先恢复它

# draft
access_inside_not_yours = 其中的{ $path }不属于您：只有其所有者才能更改其使用权限

# draft
access_mode_already = { $inside ->
        [yes] { $name }及其中所有内容的使用权限已是所要求的设置
       *[no] { $name }的使用权限已是所要求的设置
    }

# draft
access_mode_comes_to = { $mode }：{ $who }

# draft
access_no_class = { $part }未指定类别，而此系统不会像 chmod 那样说明其 umask 屏蔽了哪些权限：请指定类别，如 u{ $clause } 表示所有者，a{ $clause } 表示所有人

# draft
access_not_a_mode = { $text }不是权限模式：请按 chmod 的写法输入，如 644、u+x 或 go-w

# draft
access_not_changed = 无法更改{ $path }的使用权限

# draft
access_not_given_back = 无法将其所属组恢复为 { $group }

# draft
access_not_given_group = 无法将{ $path }的所属组设为 { $group }

# draft
access_not_in_group = 您不在组 { $group } 中

# draft
access_not_kept = 存放{ $name }的驱动器未保留此更改：其文件系统不保存权限

# draft
access_not_owner = 只有其所有者才能更改其使用权限

# draft
access_not_put_back = { $more ->
        [0] 无法恢复{ $path }的使用权限：{ $error }
       *[other] 无法恢复{ $path }（及另外 { $more } 个项目）的使用权限：{ $error }
    }

# draft
access_not_read_only = 非只读

# draft
access_not_yours = { $path }不属于您

# draft
access_nothing_asked = 没有要更改的内容：请提供权限模式或组

# draft
access_owner_group_then_others = 所有者和所属组{ $both ->
        [none] 无权使用它
       *[some] 可以{ $both_does }
    }，其他用户{ $others ->
        [none] 无权使用它
       *[some] 可以{ $others_does }
    }

# draft
access_owner_then_rest = 所有者{ $owner ->
        [none] 无权使用它
       *[some] 可以{ $owner_does }
    }，所属组和其他用户{ $rest ->
        [none] 无权使用它
       *[some] 可以{ $rest_does }
    }

# draft
access_put_back = { $more ->
        [0] 恢复{ $name }的使用权限
       *[other] 恢复{ $name }及另外 { $more } 个项目的使用权限
    }

# draft
access_put_back_already = { $more ->
        [0] { $name }的使用权限已与之前相同
       *[other] { $name }及另外 { $more } 个项目的使用权限已与之前相同
    }

# draft
access_put_back_done = { $more ->
        [0] 已恢复{ $name }的使用权限
       *[other] 已恢复{ $name }及另外 { $more } 个项目的使用权限
    }

# draft
access_read_only = 只读

# draft
access_reference = 访问权限

# draft
access_reference_600 = 仅所有者可读写

# draft
access_reference_644 = 所有者可读写；所属组和其他用户可读

# draft
access_reference_700 = 仅所有者可读、写和运行

# draft
access_reference_755 = 所有人可读和运行；所有者还可写

# draft
access_reference_also = 还可以

# draft
access_reference_big_x = 打开文件夹，并运行已有人可运行的文件

# draft
access_reference_clauses_note = 多个子句依次排列，以逗号分隔：u+x,go-w。

# draft
access_reference_digits = 数字：所有者、所属组、其他用户

# draft
access_reference_digits_note = 每个数字中，读取加 4，写入加 2，运行（对文件夹为打开）加 1。

# draft
access_reference_everyone = 所有人

# draft
access_reference_group = 所属组

# draft
access_reference_no_longer = 不能再

# draft
access_reference_only = 只能

# draft
access_reference_others = 其他用户

# draft
access_reference_owner = 所有者

# draft
access_reference_r = 读取（对文件夹为列出内容）

# draft
access_reference_s = 以其所有者或所属组的身份运行；文件夹的所属组会传给其中新建的内容

# draft
access_reference_t = 文件夹中的内容只能由其各自的所有者删除

# draft
access_reference_then = 然后

# draft
access_reference_w = 写入（对文件夹为更改其内容）

# draft
access_reference_what = 权限

# draft
access_reference_who = 对象

# draft
access_reference_x = 运行（对文件夹为打开）

# draft
access_refused = { $name }：{ $why }

# draft
access_set_gid_cleared = 系统清除了{ $name }的 set-group-ID 位：只有其所属组的成员才能设置它

# draft
access_sgid_both_file = 文件以其所属组的身份运行

# draft
access_sgid_both_file_gone = 文件不再以其所属组的身份运行

# draft
access_sgid_both_folder = 在文件夹中新建的内容会继承其所属组

# draft
access_sgid_both_folder_gone = 在文件夹中新建的内容不再继承其所属组

# draft
access_sgid_file = 它以其所属组的身份运行

# draft
access_sgid_file_gone = 它不再以其所属组的身份运行

# draft
access_sgid_folder = 在其中新建的内容会继承其所属组

# draft
access_sgid_folder_gone = 在其中新建的内容不再继承其所属组

# draft
access_sticky_both = 文件夹中的内容只能由其各自的所有者删除

# draft
access_sticky_both_gone = 凡可更改文件夹的人都可删除其中的内容

# draft
access_sticky_file = 它保留粘滞位，但文件会忽略此位

# draft
access_sticky_file_gone = 移除其粘滞位（文件会忽略此位）

# draft
access_sticky_folder = 其中的内容只能由其各自的所有者删除

# draft
access_sticky_folder_gone = 凡可更改它的人都可删除其中的内容

# draft
access_stopped_not_put_back = 已停止，且{ $not_put_back }

# draft
access_suid_both = 文件以其所有者的身份运行

# draft
access_suid_both_gone = 文件不再以其所有者的身份运行

# draft
access_suid_file = 它以其所有者的身份运行

# draft
access_suid_file_gone = 它不再以其所有者的身份运行

# draft
access_suid_folder = 它保留 set-user-ID 位，但文件夹会忽略此位

# draft
access_suid_folder_gone = 移除其 set-user-ID 位（文件夹会忽略此位）

# draft
access_then = { $said }；{ $then }

# draft
access_unreadable = 无法读取{ $path }

# draft
access_windows_each_stays = 各项保持原样：此处仅保留所有者的 w 位

# draft
app_access_group_title = 更改访问权限和所属组

# draft
app_access_nothing = 请提供权限模式、组或两者

# draft
app_access_title = 更改访问权限（{ $mode }）

# draft
app_apply_plan = 应用计划

# draft
app_apply_plan_description = 应用以数据形式给出的计划（plan）：与任何命令一样，由调用方应用它——用户自己的操作立即应用，智能体的则在审阅后应用

# draft
app_arriving = 即将到达文件夹的项目

# draft
app_arriving_description = 返回等待中和运行中的操作要带入某个文件夹（未指定时为窗格的文件夹）但尚未到达的内容（ADR-135）：每个项目在那里的名称、带来它的计划、其状态（waiting：等待中；starting：正在开始；copying：与其他项目一起复制，进度为整批的比例；moving：按其自身从 0 到 1 的进度移动；landed：已到位，其操作仍在继续）、其操作是否已暂停、它从何处复制或移动而来，以及它是文件夹还是文件、大小多少

# draft
app_back = 后退

# draft
app_back_description = 转到上一个位置

# draft
app_cancel_in = 取消文件夹中正在进行的操作

# draft
app_cancel_in_description = 取消涉及某个文件夹或其内容的正在进行或等待中的操作；已完成的部分保持不变，并且可以撤销

# draft
app_commands = 列出命令

# draft
app_commands_description = 返回每个已注册的命令及其参数架构

# draft
app_copy = 复制到…

# draft
app_copy_description = 将所选内容或给定的项目复制到某个目录（计划）；on_conflict 指定该处名称已被占用时如何处理

# draft
app_delete = 永久删除

# draft
app_delete_description = 不经回收站删除所选内容或给定的项目（计划，不可撤销）

# draft
app_delete_title = 永久删除

# draft
app_destination_path = 目标必须是路径

# draft
app_entries = 列出项目

# draft
app_entries_description = 窗格所显示的内容，按其顺序（已排序、已筛选，含就地展开的文件夹）；或指定 `location` 时，列出某个文件夹所含的内容而不显示它：每个项目的 ID、名称、路径、kind（文件、文件夹或链接）、大小和修改时间（自 1970 年起的秒数），以及是否已选中；从 `offset` 起，至多 `limit` 个。`complete: false`：列表仍在加载，请再次询问。`index`（若有）：此回答由某个索引给出（`by` 说明是哪个索引；`proof` 说明其得到证实的程度：`live` 即文件当前的实际状态，`checked` 即已于 `at` 时与其文件夹核对，`as_of` 即截至 `at` 时的内容，因此此后新建、重命名或删除的文件不在其中；`at` 以自 1970 年起的秒数表示）。`as_of`（若有）：此回答来自某个索引截至其上次更新时的内容（`by` 说明是哪个索引，`updated` 说明何时更新，以自 1970 年起的秒数表示）。`excluded`（若有）：搜索在起点之下跳过了多少文件和文件夹（设置 › 排除项中排除的文件夹、缓存、回收站、网络文件夹）；在搜索中写 `excluded:yes` 则也会遍历这些文件夹

# draft
app_expand = 展开文件夹

# draft
app_expand_description = 在所显示的文件夹下就地展开文件夹（如树状视图所绘），或将其收起

# draft
app_filter = 筛选

# draft
app_filter_description = 只显示 `text` 保留的项目：按筛选器的解读方式读取（query 插件：单词匹配名称，列表本身能判定的条件——如 kind:image、ext:pdf 或 size>1M——作用于文件，文件夹在对其搜索找到匹配项后显示），否则显示名称包含该文本的项目（忽略大小写，除非其中有大写字母）；不给 `text` 时显示全部。其中作用于文件的条件构成窗格的透镜，在窗格显示其他位置时仍保留，直到被移除；其中的单词则只属于当前位置

# draft
app_forward = 前进

# draft
app_forward_description = 返回“后退”之前所在的位置

# draft
app_group = 分组

# draft
app_group_as_sorted = 与排序方式相同

# draft
app_group_by = 按{ $key }分组

# draft
app_group_by_way = 按{ $key }分组，{ $way }

# draft
app_group_description = 将窗格中的项目分组并加上标题（ADR-130）：依据 `by`（none；same_as_sort：按排序的键及其方向；或 `sort` 所接受的键：name、kind、type、extension、size、modified、created、accessed、deleted、rating、tags、owner、folder；taken、dimensions、duration 按需从文件中读取），方向由 `ascending` 指定（每个键有其默认方向：文字从 A 到 Z，最大的、最新的、评分最高的在前；再次选择同一键则反转）。在每组内，文件夹仍像在其他地方一样（若设置如此）排在前面，然后由排序决定

# draft
app_group_fold = 折叠该组

# draft
app_group_fold_description = 折叠或展开窗格中的分组（ADR-131）：按 `mode` 处理分组 `group`（从第一个标题起从 0 计数；省略时为光标所在的组）：toggle、fold、unfold、fold_others（除它之外的所有组）、unfold_all。折叠的组中的项目不再属于窗格所显示的内容（不绘制，按键会跳过，不在“全选”和命令作用的范围内），其标题及计数保留；只要窗格仍按同一标准分组，折叠状态就会保留，且只在绘制分组标题的视图（列表、树、网格）中生效：在其他视图中，所有文件照常显示

# draft
app_group_fold_others = 折叠其他分组

# draft
app_group_fold_toggle = 折叠或展开该组

# draft
app_group_none = 不分组

# draft
app_group_title = 更改所属组（{ $group }）

# draft
app_group_unfold_all = 展开所有分组

# draft
app_history = 撤销历史记录

# draft
app_history_description = 返回“撤销”可撤回的操作，最新的在前（ADR-079，13）。每个操作给出其 `entry`（撤销和重做接收的 `entry`）、`title`、`at`：应用的时间（自 1970 年起的秒数，UTC）、其 `state`（running、waiting、done 或 undone）、`can`：其撤销（或已撤销操作的重做）现在能做什么（`now`；`once_ended`：在其运行或等待时；`after`：在其所指的操作先被撤销或重做之后；`going`：在其正被撤销或重做时；`never`），以及 `with`：指定 `with_after` 的撤销会一并撤回的项目

# draft
app_hydrate = 加载元数据

# draft
app_hydrate_description = 请求项目的元数据（视图会为可见的行调用此命令）

# draft
app_in_no_folder = { $path } 不在任何文件夹中

# draft
app_keep = 添加到侧边栏

# draft
app_keep_cannot = 此位置无法保留在侧边栏中

# draft
app_keep_description = 将窗格所显示的内容按其显示方式保留在侧边栏中（ADR-109）：文件夹作为书签，搜索作为已保存的搜索，任何经筛选显示的内容作为带其筛选条件的已保存位置

# draft
app_keep_nothing = 窗格中没有可保留的内容

# draft
app_link = 链接到…

# draft
app_link_description = 在某个目录中为所选内容或给定的每个项目创建同名链接（计划）；on_conflict 指定该处名称已被占用时如何处理

# draft
app_mkdir = 新建文件夹

# draft
app_mkdir_description = 在 `in` 所给的文件夹或窗格所显示的文件夹中创建文件夹（计划）

# draft
app_move = 移动到…

# draft
app_move_description = 将所选内容或给定的项目移动到某个目录（计划）；on_conflict 指定该处名称已被占用时如何处理

# draft
app_navigate = 转到位置

# draft
app_navigate_description = 在窗格中打开某个路径或位置：`select` 指定某个项目时，列出后选中该项目；否则，若是向上转到它，则选中返回下层时要经过的那个项目；`in_place` 将其放在窗格当前所示的步骤处，不另占一步；`as_kept` 按已保存位置的保存方式显示它，仅以 `filter` 筛选，不含窗格的透镜（ADR-109）

# draft
app_new_file = 新建文件

# draft
app_new_file_description = 在 `in` 所给的文件夹或窗格所显示的文件夹中创建一个空文件（计划）；以 / 结尾的名称会创建文件夹

# draft
app_new_file_title = 新建文件{ $name }

# draft
app_new_folder_title = 新建文件夹{ $name }

# draft
app_no_directory = 窗格显示的不是目录

# draft
app_no_pane = 没有这个窗格

# draft
app_no_pane_numbered = 没有窗格 { $pane }

# draft
app_no_provider = 没有用于 `{ $scheme }` 方案的提供程序

# draft
app_no_work_dir = 请选择一些项目，或打开一个文件夹

# draft
app_notes_read = 读取

# draft
app_notes_read_all = 它可以保存文件的全部标注。

# draft
app_notes_read_limits = 它无法保存的内容：{ $limits }。

# draft
app_notes_read_merged = 读取时，此处保存的内容会与其他位置的标注合并显示。

# draft
app_notes_write = 写入

# draft
app_notes_write_renames = 从不；或者，在文件名已含标签时，随编辑对标签的更改重命名文件。

# draft
app_notes_write_when = 从不；仅在文件已在此处有标注时；文件所在的卷不保存属性时也写入；或每个编辑过的文件都写入。

# draft
app_nothing_taken_out = 此处没有可移出的内容：“移到回收站”才会删除

# draft
app_nothing_to_redo = 没有可重做的操作

# draft
app_nothing_to_retry = 没有可重试的操作

# draft
app_nothing_to_undo = 没有可撤销的操作

# draft
app_orders_unreadable = 数据文件中的 { $key } 不是列表：请修复或删除它，以便记住顺序

# draft
app_pane_went_elsewhere = 窗格已转到别处

# draft
app_passphrase = 密码短语

# draft
app_password = 密码

# draft
app_plan_diff = 预览计划

# draft
app_plan_diff_description = 试运行计划，并返回每个步骤将会做什么

# draft
app_redo = 重做

# draft
app_redo_description = 重做最近撤销的计划，或者在其后撤销的操作均未触及其所做更改时，重做所指定的已撤销历史记录项；从窗口外请求时，说明再次应用了什么；从窗口中请求时，像“撤销”一样返回其运行所用的 ID（runs）

# draft
app_refresh = 刷新

# draft
app_refresh_description = 重新列出窗格的位置

# draft
app_relative_nowhere = { $path } 是相对路径，但没有显示可作为其所在位置的文件夹

# draft
app_remember_order = 记住此文件夹的顺序

# draft
app_remember_order_description = 记住窗格所示文件夹的排序和分组方式（ADR-132）以及显示它的视图（`view`：list 或 grid），或忘记这些（`on`；省略时切换）：记住顺序的文件夹在用户窗口的每个窗格中都如此显示（若是在应答其他程序对话框的窗口中记住的，则在每个这样的窗口的每个窗格中如此显示：对话框记住的内容另行保存，ADR-145），在其中对排序、分组或视图所做的更改会更新所记住的内容，窗格自己的顺序和视图在显示它时暂且搁置，显示其他位置时恢复；忘记后，窗格自己的设置随即恢复。其视图仅在窗格显示的是文件夹可记住的视图时才显示，绝不取代分栏或树状视图；分栏中的每一栏按其自身文件夹的顺序排列。除搜索（其条件即其本身）外，任何位置均可（回收站、标签的位置）；隐藏文件仍由窗格决定，文件夹优先是用户的设置（在对话框中是对话框自己的设置）

# draft
app_rename = 重命名

# draft
app_rename_description = 重命名所选项目或给定的项目（计划）

# draft
app_rename_one = 重命名需要恰好一个项目

# draft
app_rename_title = 重命名为{ $name }

# draft
app_retry = 重试

# draft
app_retry_description = 再次运行因错误而停止的计划所剩下的部分，未指定时为最后停止的那个；它所应用的内容随该计划一起撤销；从窗口外请求时，说明应用了什么

# draft
app_room_detail = { $title }：驱动器（尤其是网络共享）报告的可用空间可能少于实际空间。您可以仍然复制，或取消并腾出空间。

# draft
app_running = 正在运行的操作

# draft
app_running_description = 按开始顺序返回正在运行的操作：每个操作的标题、已结束的步骤、已完成的字节数和总字节数（已知时，按计划所定）、可估算时的速度（字节/秒）和剩余时间（秒），以及服务器仍在接收的文件（ADR-113）

# draft
app_search_order = 不会记住搜索的顺序：搜索由其条件决定

# draft
app_select = 选择

# draft
app_select_all = 全选

# draft
app_select_all_description = 选中所有项目

# draft
app_select_description = 按 ID 或名称选择项目

# draft
app_select_invert = 反向选择

# draft
app_select_invert_description = 选中所显示但未选中的项目，并取消选中其余项目

# draft
app_select_no_entry = 没有名为 `{ $name }` 的项目

# draft
app_select_none = 清除选择

# draft
app_select_none_description = 取消选中所有项目

# draft
app_select_not_shown = `{ $name }` 未显示：它是隐藏的，或被筛选排除

# draft
app_select_range_target = 范围需要一个目标 ID

# draft
app_select_sweep = 随光标移动选择

# draft
app_select_sweep_description = 从光标处保持一个范围：移动光标会选中（或带 `unset` 时取消选中）其间的项目

# draft
app_select_sweep_end = 停止随光标移动选择

# draft
app_select_sweep_end_description = 结束从光标处保持的范围，保留其所覆盖的选择

# draft
app_select_sweep_unset = 随光标移动取消选择

# draft
app_set_access = 更改访问权限

# draft
app_set_access_describe = 说明权限模式的作用

# draft
app_set_access_describe_description = 用文字说明 `mode`（按 chmod 的写法）会对当前模式为 `current` 的项目产生什么结果（若全部相同，则为一个数字；否则说明每个子句对每一项的作用），全部为同一类时给出 `folders` 或 `files`（否则两者都说明，不同之处分开说明）；对于不是模式的内容返回错误，与 `set_access` 拒绝它的方式相同

# draft
app_set_access_description = 更改所选内容或给定项目的读取、写入和运行权限：`mode` 按 chmod 的写法，`group` 用编号，`inside` 用于文件夹中的内容（计划）。更改的是链接所指向的目标；所有者永远不会改变

# draft
app_show_companions = 显示隐藏文件和伴随文件

# draft
app_show_companions_description = 在窗格中显示隐藏文件，并开启“显示伴随文件”（`core.show_companions`）：即在每个文件夹中，把另一个文件的标注保存在其旁边的那些文件

# draft
app_show_in_folder = 在文件夹中显示

# draft
app_show_in_folder_description = 打开所选文件所在的文件夹，并选中该文件：用于搜索、标签、暂存架等列出来自多个文件夹的文件的位置

# draft
app_show_in_folder_one = 请选择一个文件以在其文件夹中显示

# draft
app_shows_nothing = 窗格未显示任何内容

# draft
app_sort = 排序

# draft
app_sort_ascending = 升序排列

# draft
app_sort_by = 按{ $key }排序

# draft
app_sort_by_way = 按{ $key }排序，{ $way }

# draft
app_sort_descending = 降序排列

# draft
app_sort_description = 设置窗格的排序键、方向以及是否显示隐藏文件，并设置文件夹是否排在前面（`folders_first`：在应答其他程序对话框的窗口中是对话框自己的设置，否则是用户在其所有窗口中的设置，智能体的调用会先征得用户同意）

# draft
app_take_out = 从此处移出

# draft
app_take_out_description = 将所选内容从列出保存在别处的文件的位置中移出，具体如该位置所说明（标签的位置会去掉文件上的该标签，暂存架会将其取下）：文件本身留在原处。这也是 Delete 键在那里的作用

# draft
app_take_out_not_in_dialogs = 在应答其他程序对话框的窗口中不执行{ $action }

# draft
app_toast_redo = 重做

# draft
app_toast_undo = 撤销

# draft
app_toggle_hidden = 切换显示隐藏文件

# draft
app_toggle_hidden_description = 显示或隐藏以点开头的项目

# draft
app_trash = 移到回收站

# draft
app_trash_description = 将所选内容或给定的项目移到回收站（计划）

# draft
app_trash_none_kept = { $count ->
        [1] 它位于网络文件夹中，网络文件夹没有回收站
       *[other] 它们位于网络文件夹中，网络文件夹没有回收站
    }

# draft
app_trash_others_kept = { $count ->
        [1] 它位于网络文件夹中，网络文件夹没有回收站
       *[other] 它们位于网络文件夹中，网络文件夹没有回收站
    }；{ $kept ->
        [1] 另一个项目会移到回收站
       *[other] 另外 { $kept } 个项目会移到回收站
    }

# draft
app_trash_title = 移到回收站

# draft
app_undo = 撤销

# draft
app_undo_description = 撤销最近的计划，或者在其后请求的操作均未触及其所做更改时，撤销所指定的计划或历史记录项（with_after 加上一个历史记录项时：连同其后触及其更改的各项，最新的在前）；从窗口外请求时，按历史记录项的标题说明撤回了什么；从窗口中请求时，返回其运行所用的 ID（runs），以便窗口只跟踪自己的运行

# draft
app_undo_plan_or_entry = 请指定计划或历史记录项，不能同时指定两者

# draft
app_undo_with_after = with_after 会将某个历史记录项连同其后的各项一起撤回：请指定该项（entry）

# draft
app_up = 上级目录

# draft
app_up_description = 转到包含所示文件夹的文件夹，或其路径栏中的前一个位置（标签的类别、搜索所在的文件夹），并在那里选中刚离开的那个

# draft
clash_taken = { $name }已在该处

# draft
command_invalid_args = `{ $command }` 的参数无效：{ $reason }

# draft
command_nothing_selected = 未选择任何内容

# draft
command_nothing_to_paste = 没有可粘贴的内容

# draft
command_panicked = { $command }因自身故障而停止（日志中记录了位置）

# draft
command_unknown = 未知命令 `{ $command }`

# draft
command_with_args = { $title }（{ $args }）

# draft
entry_kept_elsewhere = { $count } 个文件在别处还有其他名称：磁盘上仍保留其所占的 { $size }

# draft
exclusions_agent_exposes = 智能体不能让私密文件夹变为可读（{ $folders }）：只有您可以在“设置”的“排除项”中这样做

# draft
exclusions_agent_shell = 智能体不能更改是否读取登录 shell 的变量：仅由这些变量指定的私密文件夹将不再私密。只有您可以在“设置”的“隐私”中更改

# draft
exclusions_bad_glob = { $pattern }：{ $error }

# draft
exclusions_bad_names = names 应为 files、folders 或 both，而不是 { $names }

# draft
exclusions_bad_regex = regex: { $error }

# draft
exclusions_bad_variable = { $pattern }：变量写作 $NAME、${"{"}NAME{"}"} 或 ${"{"}NAME:-未设置时的替代值{"}"}

# draft
exclusions_beside_files = { $pattern }：beside 只能用于文件夹，不能用于文件

# draft
exclusions_beside_name = { $sibling }：beside 指定的是文件或文件夹的名称，而不是路径

# draft
exclusions_beside_path = { $pattern }：在文件旁排除时，文件夹应写成名称（target），而不是路径

# draft
exclusions_builtin_id = skwir. 指的是 Skwir 自己的文件夹；请为您的文件夹取一个不含点的名称

# draft
exclusions_categories = 排除项类别

# draft
exclusions_categories_description = Skwir 的排除项类别中有哪些更改（某个用途被切换：searches、agents、index），以及您自己的类别（一个名称）

# draft
exclusions_categories_table = 文件夹类别是一个按 ID 组织的表

# draft
exclusions_category_name = 您的类别需要一个名称

# draft
exclusions_category_not_table = 类别是一个表

# draft
exclusions_empty = 文件夹需要名称或路径

# draft
exclusions_entries = 排除项

# draft
exclusions_entries_description = Skwir 的排除项中有哪些更改（某个用途被取消勾选、某个类别），以及您自己的排除项：一个模式（名称、路径、glob 或 regex:…，作用于文件、文件夹或两者，可带 beside），或 holding（文件夹中所含的某个文件），它是什么、其类别，以及它被排除在哪些用途之外

# draft
exclusions_entries_table = 排除项是一个按 ID 组织的表

# draft
exclusions_entry_not_table = 条目是一个表

# draft
exclusions_holding_alone = beside 和 names 应与 pattern 搭配，而不是与 holding 搭配

# draft
exclusions_holding_name = holding 是文件夹中所含文件的名称

# draft
exclusions_id_dot = 您的 ID 不能含点：含点的 ID 属于 Skwir 及其插件

# draft
exclusions_legacy_folders = 私密文件夹（排除项之前）

# draft
exclusions_legacy_folders_description = 0.3.1 版的私密文件夹，作为对智能体隐藏的排除项读取

# draft
exclusions_legacy_marker = 标记为私密的文件夹（排除项之前）

# draft
exclusions_legacy_marker_description = 0.3.1 版针对 .private 的开关，作为“包含 .private 文件”这一排除项读取

# draft
exclusions_names_nothing = 须由 pattern 或 holding 指定所排除的对象

# draft
exclusions_no_category = 没有类别 { $category }

# draft
exclusions_no_field = 没有字段 { $key }

# draft
exclusions_no_id = 需要一个 ID

# draft
exclusions_not_switch = { $key }：应为 true 或 false

# draft
exclusions_path_starts = { $pattern }：路径应以 ~ 或变量（$CARGO_HOME）开头，或为完整路径（{ $whole }）

# draft
exclusions_pattern_or_holding = 请指定一个模式或文件夹中所含的文件，不能同时指定两者

# draft
exclusions_ref_any_depth = 其下任意深度

# draft
exclusions_ref_beside = 仅在旁边有此项时

# draft
exclusions_ref_beside_note = 名称或名称的模式，不能是路径。

# draft
exclusions_ref_by_name = 按名称，任意位置

# draft
exclusions_ref_by_path = 按路径

# draft
exclusions_ref_cache_mark = 缓存自身的标记（带签名）

# draft
exclusions_ref_either = 任一

# draft
exclusions_ref_file_held = 其中所含的文件

# draft
exclusions_ref_from_home = 一个文件夹，从您的主文件夹算起

# draft
exclusions_ref_from_root = 一个文件夹，从根目录算起

# draft
exclusions_ref_from_variable = 从变量算起

# draft
exclusions_ref_holding = 包含

# draft
exclusions_ref_holding_named = 任何包含此名称文件的文件夹

# draft
exclusions_ref_holding_note = 文件的名称：包含它的文件夹会被排除，无论位于何处。

# draft
exclusions_ref_like_beside = 仅当旁边有名称与之相似的项时

# draft
exclusions_ref_name_beside = 旁边的名称

# draft
exclusions_ref_named_like = 任何名称与之相似的项

# draft
exclusions_ref_one = 一个字符

# draft
exclusions_ref_one_of = 其中之一

# draft
exclusions_ref_path_like = 路径与之相似的项

# draft
exclusions_ref_path_note = 路径以 ~、变量或根（{ $whole }）开头；其他都是名称。

# draft
exclusions_ref_pattern = 名称或路径

# draft
exclusions_ref_regex = 正则表达式，~ 表示主文件夹

# draft
exclusions_ref_regex_note = 写在 regex: 之后，匹配以 ~ 表示您主文件夹的路径。

# draft
exclusions_ref_signs = 名称或路径中的符号

# draft
exclusions_ref_so_named = 任何如此命名的项

# draft
exclusions_ref_so_named_beside = 仅当旁边有如此命名的文件或文件夹时

# draft
exclusions_ref_star = 任意字符，限于一个名称内

# draft
exclusions_ref_stars = 任意层文件夹，用于路径中

# draft
exclusions_ref_starts_so = 任何名称以此开头的项

# draft
exclusions_ref_variable_unset = 从变量算起，或变量未设置时的替代值

# draft
exclusions_ref_whole_path = 匹配整个路径

# draft
exclusions_refused = { $at }：{ $why }

# draft
exclusions_said_beside = 旁边有 { $sibling } 的 { $what }

# draft
exclusions_said_holding = 任何包含 { $file } 的文件夹

# draft
exclusions_said_kernel = 内核自身文件系统的挂载位置：proc、sysfs、cgroup 等

# draft
exclusions_said_like = { $names ->
        [files] 任何名称类似 { $pattern } 的文件
        [folders] 任何名称类似 { $pattern } 的文件夹
       *[both] 任何名称类似 { $pattern } 的文件或文件夹
    }

# draft
exclusions_said_like_beside = { $names ->
        [files] 任何名称类似 { $pattern } 且旁边有 { $sibling } 的文件
        [folders] 任何名称类似 { $pattern } 且旁边有 { $sibling } 的文件夹
       *[both] 任何名称类似 { $pattern } 且旁边有 { $sibling } 的文件或文件夹
    }

# draft
exclusions_said_matching = { $names ->
        [files] 匹配 { $pattern } 的文件
        [folders] 匹配 { $pattern } 的文件夹
       *[both] 匹配 { $pattern } 的文件和文件夹
    }

# draft
exclusions_said_name = { $names ->
        [files] 任何名为 { $pattern } 的文件
        [folders] 任何名为 { $pattern } 的文件夹
       *[both] 任何名为 { $pattern } 的文件或文件夹
    }

# draft
exclusions_said_name_beside = { $names ->
        [files] 任何名为 { $pattern } 且旁边有 { $sibling } 的文件
        [folders] 任何名为 { $pattern } 且旁边有 { $sibling } 的文件夹
       *[both] 任何名为 { $pattern } 且旁边有 { $sibling } 的文件或文件夹
    }

# draft
exclusions_said_now = { $pattern }，当前为 { $path }

# draft
exclusions_said_path_like = { $names ->
        [files] 路径类似 { $pattern } 的文件
        [folders] 路径类似 { $pattern } 的文件夹
       *[both] 路径类似 { $pattern } 的文件和文件夹
    }

# draft
exclusions_said_unset = { $pattern }，在 { $variables } 未设置时：无

# draft
exclusions_shell_variables = 读取 shell 的变量

# draft
exclusions_shell_variables_description = 当路径以变量开头时，也从您的登录 shell 读取 CARGO_HOME 等变量。

# draft
exclusions_shell_variables_description_more = 从桌面启动的 Skwir 看不到 .zshrc 或 config.fish 导出的变量。这些文件一旦发生变化，Skwir 会在后台询问 shell，并且只保留这些路径所用到的变量。

# draft
exclusions_unreadable = { $count ->
        [1] 有一个排除项无法读取，因此不生效：{ $first }
       *[other] 有 { $count } 个排除项无法读取，因此不生效。第一个：{ $first }
    }

# draft
exclusions_what_also = { $what }（其列表中的另一项）

# draft
exclusions_what_before = { $what }（移动之前所在的位置）

# draft
exclusions_what_cache_home = 您的缓存文件夹（XDG_CACHE_HOME）

# draft
exclusions_what_legacy = 私密（来自 0.3.1 版的列表）

# draft
exec_being_redone = { $title }已在重做中

# draft
exec_being_undone = { $title }已在撤销中

# draft
exec_cannot_be_undone = { $title }无法撤销

# draft
exec_chain_cancelled = { $title }已取消

# draft
exec_chain_did_not_run = { $title }未运行

# draft
exec_chain_stopped = { $title }已停止：{ $error }

# draft
exec_gone_from_history_redo = 该操作已不在历史记录中：没有可重做的内容

# draft
exec_gone_from_history_undo = 该操作已不在历史记录中：没有可撤销的内容

# draft
exec_not_undone = { $title }未撤销：{ $why }

# draft
exec_not_undone_nothing = { $title }未被撤销：没有可重做的内容

# draft
exec_nothing_to_redo = 没有可重做的内容

# draft
exec_nothing_to_undo = 没有可撤销的内容

# draft
exec_redo = 重做

# draft
exec_redo_held = 无法单独重做{ $title }：在它之后撤销的{ $other }触及了它所做的更改；请先重做后者

# draft
exec_redo_held_for_good = 无法重做{ $title }：在它之后撤销的{ $other }触及了它所做的更改，且其本身无法重做

# draft
exec_that_not_undone = 该操作未撤销：{ $why }

# draft
exec_under_way = 那里仍有操作正在进行：{ $actions }

# draft
exec_under_way_title = { $title }

# draft
exec_undo = 撤销

# draft
exec_undo_held = 无法单独撤销{ $title }：在它之后请求的{ $other }触及了它所做的更改；请先撤销后者

# draft
exec_undo_held_for_good = 无法撤销{ $title }：在它之后请求的{ $other }触及了它所做的更改，且其本身无法撤销

# draft
exec_undo_held_running = 无法单独撤销{ $title }：在它之后请求、正在运行的{ $other }正在更改它所做的内容；请先撤销后者

# draft
exec_undo_held_waiting = 无法单独撤销{ $title }：在它之后请求、正在等待运行的{ $other }将会更改它所做的内容；请先撤销后者

# draft
exec_undone_already = { $title }已撤销

# draft
folders_not_private = { $path } 并非仅归该用户所有

# draft
form_needed = { $field }为必填项

# draft
form_one_of_choices = { $field }须为其选项之一

# draft
form_whole_number = { $field }须为 { $min } 到 { $max } 之间的整数

# draft
fs_folder_gone = 此文件夹已被删除或不再可用。

# draft
fs_not_a_path = 不是路径：{ $location }

# draft
fs_not_regular = 不是普通文件

# draft
fs_server_no_xattrs = 此服务器不保存扩展属性

# draft
fs_size_bytes = { $n } B

# draft
fs_size_gb = { $n } GB

# draft
fs_size_gib = { $n } GiB

# draft
fs_size_kb = { $n } kB

# draft
fs_size_kib = { $n } KiB

# draft
fs_size_mb = { $n } MB

# draft
fs_size_mib = { $n } MiB

# draft
fs_size_pb = { $n } PB

# draft
fs_size_pib = { $n } PiB

# draft
fs_size_tb = { $n } TB

# draft
fs_size_tib = { $n } TiB

# draft
fs_sync_not_yet = { $system } 上尚不支持将数据直写到驱动器

# draft
fs_system_no_hidden = { $system } 不保存隐藏标记

# draft
fs_system_no_xattrs = { $system } 不保存扩展属性

# draft
fs_taken = 它正被移动或删除

# draft
fs_too_large = 文件过大

# draft
fs_volume_no_hidden = 其所在卷不保存隐藏标记

# draft
inspection_mesh_colours = 网格的 { $colours } 个颜色对应 { $positions } 个位置

# draft
inspection_mesh_empty = 网格中没有三角形

# draft
inspection_mesh_index = 网格的索引 { $index } 不对应任何位置（共有 { $count } 个位置）

# draft
inspection_mesh_infinite = 网格中有一个位置不是有限值

# draft
inspection_mesh_partial = 网格的索引不能组成完整的三角形

# draft
inspection_notices = { $before } · { $after }

# draft
io_bad_name = 名称太长，或在该处不允许使用

# draft
io_busy = 它正在使用中

# draft
io_denied = 权限被拒绝

# draft
io_full = 驱动器已满

# draft
io_gone = 它已不存在

# draft
io_is_folder = 它是文件夹

# draft
io_not_empty = 文件夹不是空的

# draft
io_not_folder = 其路径中的某一部分不是文件夹

# draft
io_quota = 磁盘配额已用完

# draft
io_read_only = 驱动器为只读

# draft
io_share_gone = 网络共享已断开

# draft
io_taken = 该处已有同名项目

# draft
io_timed_out = 驱动器未及时响应

# draft
io_too_large = 文件对该驱动器而言过大

# draft
keymap_add_shortcut = 添加快捷键

# draft
keymap_args = 参数

# draft
keymap_args_description = 命令的参数，以 JSON 对象表示

# draft
keymap_args_not_json = { $keys } 的参数不是 JSON：{ $error }

# draft
keymap_args_object = { $keys } 的参数应为一个对象：{ $example }

# draft
keymap_command = 命令

# draft
keymap_command_description = 按键所运行的命令，使用命令面板中命令注册时的名称：mkdir、ui.jump、tab.new。留空则释放这些按键

# draft
keymap_keys = 按键

# draft
keymap_keys_description = 一个键加上其修饰键，或依次按下的多个键：ctrl-shift-n、g h、shift-g

# draft
keymap_needs_keys = 快捷键需要指定按键

# draft
keymap_no_key = { $stroke }不是任何按键：Ctrl 加减号键应写作 ctrl--

# draft
keymap_no_modifier = { $stroke }中的{ $modifier }不是修饰键：请使用 ctrl、alt、shift 或 super

# draft
keymap_refused = { $keys }：{ $error }

# draft
keymap_shortcuts = 您的快捷键

# draft
keymap_shortcuts_description = 您绑定的按键，优先于 Skwir 及其插件设置的按键。

# draft
keymap_shortcuts_description_more = 将命令留空即可释放按键。

# draft
keymap_unknown_command = { $keys } 运行 { $command }，但没有这个名称的命令（其插件是否已启用？）

# draft
keymap_where = 作用范围

# draft
keymap_where_description = 按键生效的位置：留空表示任何位置，pane 表示文件列表，还可加上 selection、single、dir、file、folder 等条件

# draft
name_quote_close = ”

# draft
name_quote_open = “

# draft
notes_agree = 使{ $name }的标注保持一致

# draft
notes_agree_command = 使标注保持一致

# draft
notes_agree_description = 将给定项目或所选内容从各个存储中读取的标注，写入每个保存它们的存储，使不一致的存储重新一致（计划）

# draft
notes_agree_title = 使标注保持一致

# draft
notes_agreed = 已使{ $name }的标注保持一致

# draft
notes_agreed_already = { $name }的标注已经一致

# draft
notes_and = { $first }，并{ $then }

# draft
notes_changed_since = { $what } 的标注此后已在别处被更改

# draft
notes_comment_command = 注释

# draft
notes_comment_description = 设置给定项目或所选内容的注释，或以空文本删除注释（计划）

# draft
notes_comment_title = 设置注释

# draft
notes_commented = 已设置{ $name }的注释

# draft
notes_commented_already = { $name }已有该注释

# draft
notes_gone = { $name }已不存在

# draft
notes_kept_nowhere = { $path }：其所在卷不保存标注，也未设置任何存储在其旁边保存标注

# draft
notes_no_folder = { $path } 没有所在的文件夹

# draft
notes_no_stores = 没有可用于写入的标注存储

# draft
notes_put_back = 恢复{ $name }的标注

# draft
notes_put_back_already = { $name }的标注已恢复原样

# draft
notes_put_back_done = 已恢复{ $name }的标注

# draft
notes_put_back_failed = 无法恢复 { $path } 的标注

# draft
notes_rate = 将{ $name }评为 { $stars }

# draft
notes_rate_command = 评分

# draft
notes_rate_description = 为给定项目或所选内容评 1 到 5 星，或以 0 删除其评分（计划）

# draft
notes_rate_none_title = 删除评分

# draft
notes_rate_title = 评为 { $stars }

# draft
notes_rated = 已将{ $name }评为 { $stars }

# draft
notes_rated_already = { $name }的评分已是 { $stars }

# draft
notes_rating_range = 评分范围为 1 到 { $max }

# draft
notes_rating_stars = 评分范围为 1 到 5 星

# draft
notes_refused = { $path }：{ $why }

# draft
notes_rename_tag = 将{ $name }上的标签{ $tag }重命名为{ $to }

# draft
notes_renamed_tag = 已将{ $name }上的标签{ $tag }重命名为{ $to }

# draft
notes_renamed_tag_already = { $name }上的标签{ $tag }已是{ $to }

# draft
notes_set_comment = 设置{ $name }的注释

# draft
notes_stars = { $whole } 星

# draft
notes_stars_half = { $whole ->
        [0] ½ 星
       *[other] { $whole }½ 星
    }

# draft
notes_tag = 为{ $name }添加标签{ $tags }

# draft
notes_tagged = 已为{ $name }添加标签{ $tags }

# draft
notes_tagged_already = { $name }已有标签{ $tags }

# draft
notes_take_tag = 从{ $name }移除标签{ $tag }

# draft
notes_take_tags = 从{ $name }移除标签{ $tags }

# draft
notes_took_tag = 已从{ $name }移除标签{ $tag }

# draft
notes_took_tag_already = { $name }上已没有标签{ $tag }

# draft
notes_took_tags = 已从{ $name }移除标签{ $tags }

# draft
notes_took_tags_already = { $name }上已没有标签{ $tags }

# draft
notes_uncomment = 删除{ $name }的注释

# draft
notes_uncomment_title = 删除注释

# draft
notes_uncommented = 已删除{ $name }的注释

# draft
notes_uncommented_already = { $name }原本就没有注释

# draft
notes_unrate = 删除{ $name }的评分

# draft
notes_unrated = 已删除{ $name }的评分

# draft
notes_unrated_already = { $name }原本就没有评分

# draft
notes_untag = 删除{ $name }的标签

# draft
notes_untagged = 已删除{ $name }的标签

# draft
notes_untagged_already = { $name }原本就没有标签

# draft
notes_write_create = 创建

# draft
notes_write_failed = 无法写入 { $path } 的标注

# draft
notes_write_fallback = 缺少属性时

# draft
notes_write_off = 关闭

# draft
notes_write_update = 更新已有的

# draft
op_all_in_trash = 全部内容都在回收站中

# draft
op_already_there = { $name }已在该处

# draft
op_attribute_name_not_utf8 = { $name }的某个属性名称不是 UTF-8，复制所用的属性接口无法保留它

# draft
op_attributes_not_kept = 复制时无法保留标签和其他属性：{ $folder } 不保存属性，也未设置在其旁边保存属性；若要不带属性复制，请在“设置”中关闭{ $setting }

# draft
op_attributes_refused = 复制时无法保留标签和其他属性：请检查目标位置是否保存扩展属性并允许您写入，或在“设置”中关闭{ $setting }

# draft
op_attributes_unread = 无法读取{ $name }的标签和属性

# draft
op_batch_cancelled = 已取消

# draft
op_batch_copying = { $transfer } 正在复制 { $count } 个项目

# draft
op_batch_link = { $path }是链接

# draft
op_batch_read_short = { $path }：只读取了 { $bytes } 字节中的 { $read } 字节

# draft
op_change_copy = 复制 { $from } -> { $to }

# draft
op_change_create = 创建 { $path }

# draft
op_change_delete = 删除 { $path }

# draft
op_change_link = 链接 { $link } -> { $target }

# draft
op_change_metadata = 设置 { $path } 的 { $field }

# draft
op_change_mkdir = mkdir { $path }

# draft
op_change_move = 移动 { $from } -> { $to }

# draft
op_change_remove = 移除 { $path }

# draft
op_change_rename = 重命名 { $from } -> { $to }

# draft
op_change_restore = 恢复 { $path }

# draft
op_change_swap = 交换 { $a } <-> { $b }

# draft
op_change_trash = 移到回收站 { $path }

# draft
op_changed_after_copy = { $name }在复制到{ $folder }后发生了更改：其原件留在原处

# draft
op_copied_into_itself = 文件夹不能复制到其自身中

# draft
op_copied_onto_itself = 它会被复制到其自身上

# draft
op_copy_stopped = 复制已停止

# draft
op_copy_title = { $count ->
        [1] 将{ $name }复制到 { $folder }
       *[other] 将 { $count } 个项目复制到 { $folder }
    }

# draft
op_copy_went_missing = 副本{ $name }在创建时就消失了

# draft
op_device_not_copied = { $name }是设备，无法复制

# draft
op_entry_gone = { $name }已不存在

# draft
op_entry_replaced = { $name }现在是另一个文件

# draft
op_folder_attribute_not_restored = 无法恢复文件夹的属性{ $attribute }

# draft
op_folder_attributes_differ = { $name }已存在，但文件夹属性不同

# draft
op_folder_not_made = 无法创建文件夹{ $name }

# draft
op_folder_not_removed = 无法移除文件夹{ $name }

# draft
op_folder_path_not_made = 无法创建文件夹{ $path }

# draft
op_foreign_folder = 此文件夹属于其他用户，其权限会使“撤销”无法恢复其中的内容

# draft
op_holds_mount_point = { $name }中含有{ $point }，那里挂载着驱动器或远程位置：请先弹出它

# draft
op_holes_not_yet = { $system } 上尚不支持复制含空洞的文件（稀疏文件）

# draft
op_identity_unread = 无法读取该项目的标识

# draft
op_in_trash_already = { $name }已在回收站中

# draft
op_in_trash_restore_first = { $name }在回收站中：请先恢复它

# draft
op_left_after_removal = { $more ->
        [0] 还剩下{ $path }
       *[other] 还剩下{ $path }及另外 { $more } 个项目
    }

# draft
op_left_parked = { $name }仍以{ $parked }为名

# draft
op_link_elsewhere = { $name }现在指向别处

# draft
op_link_title = { $count ->
        [1] 在 { $folder } 中创建指向{ $name }的链接
       *[other] 在 { $folder } 中创建指向 { $count } 个项目的链接
    }

# draft
op_made_beside_other = { $companion }位于{ $name }旁边，但并非与其一同创建：“撤销”会保留它们

# draft
op_made_changed = { $name }在创建后已被更改：“撤销”会保留它

# draft
op_made_companion_changed = { $companion }在创建后已被更改：“撤销”会保留它和{ $name }

# draft
op_mount_point = { $name }是挂载驱动器或远程位置的地方：请弹出它，而不是删除它

# draft
op_move_original_changed = { $name }留在原处，且在移动后已被更改：“撤销”会保留它及其副本

# draft
op_move_title = { $count ->
        [1] 将{ $name }移动到 { $folder }
       *[other] 将 { $count } 个项目移动到 { $folder }
    }

# draft
op_moved_into_itself = 文件夹不能移动到其自身中

# draft
op_moved_stays = 已移动的内容仍留在原处

# draft
op_moves_nothing = 此设备（相机）无法在其文件夹之间移动文件：只能在其中创建副本

# draft
op_name_character = 名称不能包含{ $character }

# draft
op_name_control = 名称不能包含控制字符

# draft
op_name_device = { $name }在 Windows 上是设备名称

# draft
op_name_dots = { $name }不能用作名称

# draft
op_name_empty = 名称不能为空

# draft
op_name_end = 名称不能以点或空格结尾

# draft
op_name_nul = 名称不能包含 NUL 字符

# draft
op_name_slash = 名称不能包含“/”

# draft
op_no_folder_to_go_in = 要放入的文件夹不存在

# draft
op_no_longer_a_file = { $name }不再是文件

# draft
op_no_longer_a_link = { $name }不再是链接

# draft
op_no_longer_empty = { $name }不再是空的

# draft
op_no_longer_in_trash = { $name }已不在回收站中

# draft
op_no_longer_there = { $name }已不在该处

# draft
op_no_record = { $name }不是回收站中某个项目的记录

# draft
op_no_room = { $folder }中空间不足：需要 { $needs }，可用 { $free }

# draft
op_no_trash_s = { $name }未指向回收站中的任何项目

# draft
op_no_trash_there = { $name }位于由程序挂载的网络文件夹中，该处没有回收站

# draft
op_not_a_file = { $name }不是文件

# draft
op_not_a_folder = { $name }不是文件夹

# draft
op_not_all_deleted = 无法完全删除{ $name }

# draft
op_not_all_removed = 无法完全移除{ $name }

# draft
op_not_copied = 无法复制{ $name }

# draft
op_not_empty = { $name }不是空的

# draft
op_not_in_trash = { $name }不在回收站中

# draft
op_not_linked = 无法在{ $folder }中创建指向{ $name }的链接

# draft
op_not_made = 无法创建{ $name }

# draft
op_not_moved = 无法将{ $name }移动到{ $folder }

# draft
op_not_moved_as = 无法将{ $name }移动到{ $folder }并命名为{ $new }

# draft
op_not_placed = 无法将{ $name }放入{ $folder }

# draft
op_not_removed = 无法移除{ $name }

# draft
op_not_renamed = 无法将{ $name }重命名为{ $new }

# draft
op_not_restored = 无法恢复{ $name }

# draft
op_not_served = 该传输方式不支持这些文件夹

# draft
op_not_swapped = 无法交换{ $a }和{ $b }的名称

# draft
op_not_trashed = 无法将{ $name }移到回收站

# draft
op_not_written_out = 无法将副本写入驱动器

# draft
op_original_stays = { $name }已完整复制到{ $folder }，但其原件未能全部移除

# draft
op_original_stays_path = { $path }已完整复制到{ $folder }，但其原件未能全部移除

# draft
op_panicked = 它意外失败了

# draft
op_part_left = 副本的一部分留在了{ $path }中（{ $why }）

# draft
op_partly_restored_folder = 部分恢复的文件夹仍在{ $path }

# draft
op_path_climbs = { $path }包含“..”

# draft
op_path_missing = { $path } 不存在

# draft
op_path_no_name = { $path }没有名称

# draft
op_pipe_not_made = 无法创建管道{ $name }

# draft
op_place_taken_in_trash = 另一个{ $name }已在回收站中占据了它的位置

# draft
op_plan_steps_kind = 计划须逐一列出其每个步骤：`{ $kind }` 不是单个步骤

# draft
op_read_short = 只读取了 { $bytes } 字节中的 { $read } 字节

# draft
op_record_unread = 无法读取回收站记录{ $name }

# draft
op_record_unwritten = 无法写入回收站记录{ $name }

# draft
op_redo_title = 重做{ $title }

# draft
op_refused_on_close = 其所在的驱动器或服务器在关闭文件时拒绝了它

# draft
op_rename_taken = { $name }已在该处

# draft
op_renames_duplicate = 会有多个项目被命名为 { $path }

# draft
op_socket_not_copied = { $name }是套接字，无法复制

# draft
op_spellings_undistinguished = 无法区分此文件各个硬链接名称的写法

# draft
op_stopped_being_read = 文件读取已停止

# draft
op_swap_windows = Windows 无法一步交换两个名称

# draft
op_there_again = 该处又出现了{ $name }：请先将其重命名或移走

# draft
op_there_by_earlier_step = { $name }已在该处，由之前的步骤放置

# draft
op_there_not_a_folder = { $name }已在该处，且不是文件夹

# draft
op_time_not_given = 无法为{ $path }设置时间

# draft
op_trash_copy_left = 它的一个副本留在了{ $path }中（{ $why }）

# draft
op_trash_copy_not_removed = 无法完全移除回收站中{ $name }的副本

# draft
op_trash_folder_gone = { $name }原先所在的回收站文件夹已不存在

# draft
op_trash_unread = 无法读取回收站：{ $error }

# draft
op_trash_unread_context = 无法读取回收站

# draft
op_undo_title = 撤销{ $title }

# draft
op_unknown_kind = 未知的操作类型 `{ $kind }`

# draft
op_unreadable = 无法读取{ $name }：{ $error }

# draft
op_went_missing_restored = { $name }在恢复时就消失了

# draft
op_would_stay_in_trash = { $name }会仍留在回收站中

# draft
op_write_out_stopped = 写入驱动器已停止

# draft
orders_dialog_order_state = 应答其他程序对话框的窗口列出文件的顺序，即最后一个对话框留下的顺序（ADR-145）：{ $fields }

# draft
orders_kept = 各位置记住的顺序（ADR-132），每条包含 { $fields }：在每个窗格中都如此显示

# draft
orders_kept_dialogs = 各位置在应答其他程序对话框的窗口中记住的顺序（ADR-145），每条包含 { $fields }：与用户窗口记住的分开

# draft
orders_not_saved = 未能记住该顺序：{ $error }

# draft
os_path_not_absolute = { $path } 不是绝对路径

# draft
permissions_names_private = 这指向一个私密文件夹，Skwir 不允许智能体访问它（设置，隐私）

# draft
permissions_network = 使用网络

# draft
permissions_programs = 启动程序

# draft
permissions_reaches_machine = 查找 { $path } 会使系统访问另一台计算机，智能体不得擅自这样做：须由用户先在 Skwir 中打开它

# draft
permissions_read = 读取文件内容

# draft
permissions_write = 更改文件

# draft
playing_not_answering = 播放器没有响应

# draft
playing_nothing = 没有正在播放的内容

# draft
playing_now = 正在播放的内容

# draft
playing_now_description = 正在播放的曲目、其状态和播放位置，或 null

# draft
playing_request = 控制播放器

# draft
playing_request_description = 要求正在播放的内容播放、暂停、停止、跳到下一首或上一首曲目、定位、更改音量或速度、选择影片的音轨或字幕，或全屏显示；播放器下次唤醒时执行

# draft
playing_senseless = 此请求自相矛盾

# draft
plugins_browsing = 浏览和查找

# draft
plugins_desktop = 桌面集成

# draft
plugins_files = 文件处理

# draft
plugins_list = 列出插件

# draft
plugins_list_description = 每个插件及其标题、描述和启用状态

# draft
plugins_not_saved = 未能保存插件选择：{ $error }

# draft
plugins_previews = 预览和缩略图

# draft
plugins_set_enabled = 启用或禁用插件

# draft
plugins_set_enabled_description = 在运行时切换插件并记住该选择；窗口打开时，完成会通过一次唤醒和 `plugins.list` 告知

# draft
plugins_unknown = 未知插件：{ $id }

# draft
preview_changed = { $path } 在显示时被更改

# draft
print_no_page = 未指定页码

# draft
print_not_a_page = { $text }不是页码

# draft
print_pages_backwards = { $range } 的顺序颠倒了

# draft
print_pages_from_one = 页码从 1 开始

# draft
print_paper_legal = Legal

# draft
print_paper_letter = Letter

# draft
properties_content = 内容

# draft
properties_general = 常规

# draft
properties_location = 位置

# draft
properties_more = 更多

# draft
properties_notes = 标注

# draft
properties_permissions = 权限

# draft
properties_selection = 所选内容

# draft
properties_shared_permissions = 共同权限

# draft
properties_source = 来源

# draft
properties_times = 时间

# draft
properties_volume = 卷

# draft
reference_any = 任意字符

# draft
reference_any_times = 前面的内容，任意次或零次

# draft
reference_boundary = 单词的开头或结尾

# draft
reference_case = 从此处起，不区分大小写

# draft
reference_digit = 一个数字

# draft
reference_either = 任一

# draft
reference_end = 结尾

# draft
reference_escaped = 符号本身：\. \( \[ \* \+ \? \$ \^ \|

# draft
reference_group = 分组，其后的符号作用于整个分组：(ab)+

# draft
reference_lazy = 尽可能少的次数（*? 同理）

# draft
reference_maybe = 前面的内容，或无

# draft
reference_named_group = 带名称的分组：year

# draft
reference_none_of = 除这些以外的任意一个

# draft
reference_once_or_more = 前面的内容，一次或多次

# draft
reference_one_of = 其中之一

# draft
reference_pattern = 模式

# draft
reference_pattern_note = 不支持环视和反向引用：(?=…) 和 \1 不会被解析。

# draft
reference_range = 此范围内的一个

# draft
reference_space = 任意空白：空格、制表符、行尾或其他

# draft
reference_start = 开头

# draft
reference_times = 前面的内容，2 到 4 次

# draft
reference_word_char = 字母、数字或 _

# draft
registry_compress = 压缩

# draft
registry_group_bookmarks = 书签

# draft
registry_group_devices = 设备

# draft
registry_group_network = 网络

# draft
registry_group_places = 位置

# draft
registry_group_recent_folders = 最近的文件夹

# draft
registry_group_saved = 已保存

# draft
registry_group_skwir_tabs = Skwir 标签页

# draft
registry_group_tabs = 标签页

# draft
registry_group_tags = 标签

# draft
registry_open_folder_with = 文件夹打开方式

# draft
registry_open_with = 打开方式

# draft
registry_reopen_not_below = { $path }不是重新打开的文件夹中的文件夹

# draft
registry_tags = 标签

# draft
settings_apart_own = { $file ->
        [state] 这是窗口状态，而不是设置：Skwir 读取的是它自己状态文件中的内容，从不读取此处
       *[data] 这是您在 Skwir 中创建的内容（已保存的搜索、固定的标签、标签颜色），而不是设置：Skwir 读取的是它自己数据文件中的内容，从不读取此处
    }

# draft
settings_apart_until = { $file ->
        [state] 这是窗口状态，而不是设置：仅在 Skwir 的状态文件中还没有它时才从此处读取
       *[data] 这是您在 Skwir 中创建的内容（已保存的搜索、固定的标签、标签颜色），而不是设置：仅在 Skwir 的数据文件中还没有它时才从此处读取
    }

# draft
settings_apply = 保存设置

# draft
settings_apply_description = 更改偏好设置，每个键使用其在“设置”中那一行所接受的文本；要么全部保存，要么都不保存

# draft
settings_check = 检查设置

# draft
settings_check_description = 将设置文件或以表形式给出的设置与此版本 Skwir 声明的设置进行核对：若有错误（设置不接受的值、没有对应设置的键）则失败并说明问题，并注明其不认识的所有者的键

# draft
settings_check_invalid = { $path }无效：{ $problem }

# draft
settings_check_not_settings = 不是设置：{ $error }

# draft
settings_check_one = 请提供文件或设置，两者之一

# draft
settings_check_unreadable = 无法读取{ $path }：{ $why }

# draft
settings_choice_now = { $choice }（{ $now }）

# draft
settings_choose_one = 请从 { $choices } 中选择一个

# draft
settings_close_split = 关闭的窗格

# draft
settings_close_split_active = 当前

# draft
settings_close_split_description = 使用“分屏”或 F3 关闭分屏时，哪个窗格关闭。

# draft
settings_close_split_description_more = “另一个”保留您所在的窗格。“当前”关闭您所在的窗格，保留另一个。“右侧”总是关闭右侧窗格，窗格上下排列时则关闭下方的窗格。

# draft
settings_close_split_other = 另一个

# draft
settings_close_split_right = 右侧

# draft
settings_drop_asks = 询问如何处理拖放的文件

# draft
settings_drop_asks_description = 未按住任何键将文件拖放到文件夹中时，会打开一个菜单：将其移动、复制或链接到该处。

# draft
settings_drop_asks_description_more = 关闭此项后，同一驱动器内的文件会被移动，拖到其他驱动器则会被复制。无论哪种情况，Shift 移动，Ctrl 复制，Ctrl+Shift 创建链接。

# draft
settings_expected_boolean = 应为 true 或 false

# draft
settings_expected_finite = 应为有限数值

# draft
settings_expected_json = 应为 JSON：{ $error }

# draft
settings_expected_kind = 应为{ $want ->
        [integer] 整数
        [float] 数字
        [boolean] 布尔值（true 或 false）
        [datetime] 日期
        [array] 列表
        [table] 表
       *[string] 文本
    }，而不是{ $got ->
        [integer] 整数
        [float] 数字
        [boolean] 布尔值（true 或 false）
        [datetime] 日期
        [array] 列表
        [table] 表
       *[string] 文本
    }

# draft
settings_expected_number = 应为数字

# draft
settings_expected_records = 应为记录列表：{ $error }

# draft
settings_expected_whole_number = 应为整数

# draft
settings_field_name_value_pairs = { $title }应为由名称和值组成的配对列表

# draft
settings_field_needed = { $title }为必填项

# draft
settings_field_pairs = { $title }应为配对列表

# draft
settings_field_pairs_text = { $title }：名称和值均为文本

# draft
settings_field_range = { $title }：请输入 { $min } 到 { $max } 之间的整数

# draft
settings_field_secret = { $title }保存在密钥环中，而不是设置中

# draft
settings_field_text = { $title }应为文本

# draft
settings_field_value_needs_name = { $title }：值需要有名称

# draft
settings_field_whole_number = { $title }应为整数

# draft
settings_file_invalid = { $path }无效（{ $problem }）：修复之前不会保存任何内容覆盖它

# draft
settings_file_not_text = { $path }不是文本

# draft
settings_file_unreadable = 无法读取{ $path }：{ $why }

# draft
settings_folders_first = 文件夹优先

# draft
settings_folders_first_description = 将文件夹列在文件之前，两者各自排序。

# draft
settings_folders_first_description_more = 关闭后两者一起排序：按修改时间排序时，最新的排在最前，不论是文件夹还是文件。按大小排序时，文件夹不计算大小，视为大小为零。

# draft
settings_invalid_key = 设置键无效

# draft
settings_keep_attributes = 复制时保留标签和属性

# draft
settings_keep_attributes_description = 副本会带上源文件的标签、评分、注释及其下载来源。

# draft
settings_keep_attributes_description_more = 若目标位置不保存属性，则像 Mac 那样将其保存在副本旁边；只有在无处可保存时，复制才会失败。关闭此项可不带属性复制。

# draft
settings_key_not_json = { $key }：应为 JSON：{ $error }

# draft
settings_language = 语言

# draft
settings_language_automatic = 自动

# draft
settings_language_description = Skwir 窗口所使用的语言。

# draft
settings_language_description_more = “自动”会使用您的系统语言中第一个 Skwir 已翻译的语言，否则使用英语；仍为草稿的翻译会注明这一点。标为不完整的语言只有在您选择时才会使用，其缺少的部分以英语显示。无论您选择哪种语言，智能体和脚本都以英语获得回答。

# draft
settings_language_draft = { $language } — 草稿，已审阅 { $reviewed }%

# draft
settings_language_incomplete = { $language } — 不完整，已翻译 { $translated }%，已审阅 { $reviewed }%

# draft
settings_language_pseudo = 伪语言（用于测试）

# draft
settings_language_pseudo_rtl = 伪语言，从右到左（用于测试）

# draft
settings_language_self = 简体中文

# draft
settings_last_location = 窗口上次关闭时显示的文件夹

# draft
settings_managed = { $key } 由其所属的命令管理

# draft
settings_no_plugin = 此处没有插件{ $id }：可能由外部插件或未内置的插件使用

# draft
settings_no_such = 没有这个设置

# draft
settings_not_file_value = 不是文件可保存的值：{ $error }

# draft
settings_not_text = 它不是文本

# draft
settings_other_choice = 不是 Skwir 在此处已知的选项：可能属于 { $which }

# draft
settings_owner_unknown = 此处没有任何内容声明{ $owner }：可能是已关闭的插件或外部插件

# draft
settings_parent_not_table = { $key }：上级不是表

# draft
settings_plugin_table = 应为表：enabled = true 或 false

# draft
settings_problems = { $first }{ $then }

# draft
settings_program_alone = 请只填写程序本身，不带参数：“bash”，而不是“bash -l”

# draft
settings_range = 请输入 { $min } 到 { $max } 之间的整数

# draft
settings_read_only = { $file ->
        [state] 状态文件
        [data] 数据文件
       *[settings] 设置文件
    }为只读，因此不会保留此更改

# draft
settings_read_only_link = 设置文件是指向一个无法写入的文件（例如 Home Manager 的文件）的链接，因此不会保留此更改：请在声明它的地方更改

# draft
settings_read_only_link_apart = { $file ->
        [state] 状态文件
       *[data] 数据文件
    }是指向一个无法写入的文件的链接，因此不会保留此更改

# draft
settings_record_needs_id = 每条记录都需要一个 ID

# draft
settings_records_share_id = 两条记录使用了相同的 ID { $id }

# draft
settings_renamed = 现为{ $new }：Skwir 会将其移到那里

# draft
settings_renamed_both = 未读取：已设置{ $new }，Skwir 读取的是它

# draft
settings_save_did_not_run = 设置保存未运行

# draft
settings_saving_already = 设置已在保存中

# draft
settings_section_actions = 操作

# draft
settings_section_address = 地址

# draft
settings_section_audio = 音频

# draft
settings_section_books = 图书

# draft
settings_section_code = 代码

# draft
settings_section_commands = 命令

# draft
settings_section_documents = 文档

# draft
settings_section_drag_and_drop = 拖放

# draft
settings_section_git = Git

# draft
settings_section_grammars = 语法

# draft
settings_section_grid = 网格

# draft
settings_section_language = 语言

# draft
settings_section_list_columns = 列表列

# draft
settings_section_maps = 地图

# draft
settings_section_models_3d = 3D 模型

# draft
settings_section_names_index = 文件名索引

# draft
settings_section_opening_addresses = 打开地址

# draft
settings_section_pages = 页面

# draft
settings_section_pictures = 图片

# draft
settings_section_playback = 播放

# draft
settings_section_previews = 预览

# draft
settings_section_servers = 服务器

# draft
settings_section_sizes = 大小

# draft
settings_section_sorting = 排序

# draft
settings_section_split = 分屏

# draft
settings_section_spreadsheets = 电子表格

# draft
settings_section_startup = 启动

# draft
settings_section_tabs = 标签页

# draft
settings_section_tags = 标签

# draft
settings_section_terminal = 终端

# draft
settings_section_text = 文本

# draft
settings_section_thumbnails = 缩略图

# draft
settings_section_video = 视频

# draft
settings_show_companions = 显示伴随文件

# draft
settings_show_companions_description = 显示隐藏文件时，同时列出那些把另一个文件的标注保存在其旁边的文件：Mac 的 ._ 文件、XMP 和 TagSpaces 的附属文件。

# draft
settings_show_companions_description_more = 无论如何，“属性”都会列出文件的伴随文件。

# draft
settings_size_units = 单位

# draft
settings_size_units_binary = 二进制

# draft
settings_size_units_decimal = 十进制

# draft
settings_size_units_description = 大小的计算方式。

# draft
settings_size_units_description_more = “十进制”将 1 千字节计为 1000 字节（kB、MB、GB），与磁盘销售时以及 macOS 和 GNOME 显示大小的方式相同；“二进制”计为 1024 字节（KiB、MiB、GiB），与 KDE 相同。无论哪种方式，搜索都将 10M 视为以 1024 为基数，10MB 以 1000 为基数，10MiB 以 1024 为基数。

# draft
settings_startup = 启动时打开

# draft
settings_startup_description = Skwir 启动且命令行中未指定文件夹时打开的内容。

# draft
settings_startup_folder = 启动文件夹

# draft
settings_startup_folder_choice = 选定的文件夹

# draft
settings_startup_folder_description = 在启动时打开选定的文件夹时使用。

# draft
settings_startup_folder_description_more = 留空则使用您的主文件夹。

# draft
settings_startup_last = 上次未关闭的窗口和标签页

# draft
settings_toml_on_line = { $problem }（第 { $line } 行）

# draft
settings_unit_megabytes = { $n } MB

# draft
settings_unit_percent = { $n }%

# draft
settings_unit_seconds = { $n } 秒

# draft
settings_unknown = 未知设置：{ $key }

# draft
settings_unreadable_kept = { $file ->
        [data] 无法读取您已保存的搜索、固定的标签和标签颜色（{ $why }），因此 Skwir 启动时未加载它们；在修复或删除{ $path }之前，不会向其中保存任何内容（{ $error }）。
       *[settings] 无法读取您的设置（{ $why }），因此 Skwir 以默认设置启动；在修复或删除{ $path }之前，不会向其中保存任何内容（{ $error }）。
    }

# draft
settings_unreadable_set_aside = { $file ->
        [data] 无法读取您已保存的搜索、固定的标签和标签颜色（{ $why }），因此 Skwir 启动时未加载它们。该文件已被重命名为{ $aside }并保留。
       *[settings] 无法读取您的设置（{ $why }），因此 Skwir 以默认设置启动。该文件已被重命名为{ $aside }并保留。
    }

# draft
settings_unreadable_unsaved = { $file ->
        [data] 无法读取您已保存的搜索、固定的标签和标签颜色（{ $why }），因此 Skwir 启动时未加载它们，本次也不会向其中保存任何内容。
       *[settings] 无法读取您的设置（{ $why }），因此 Skwir 以默认设置启动，本次也不会向其中保存任何内容。
    }

# draft
shrink_lossless = 无损

# draft
shrink_note_damaged = 它已损坏，精简副本会丢失阅读器在显示时所修复的内容

# draft
shrink_note_date_not_kept = 无法保留其日期，因此已将其略去。

# draft
shrink_note_date_part_unread = 无法读取其日期的一部分，因此已将其略去。

# draft
shrink_note_exif_unread = 无法读取其 EXIF，因此已略去其日期。

# draft
shrink_note_pixels_as_stored = 其像素按存储时的样子保留，仍像之前一样按其 EXIF 摆正方向。

# draft
shrink_note_pixels_kept = 其像素保持原样：重新写入后反而更大。

# draft
shrink_note_profile_left_out = 已略去其颜色配置文件：它不适用于副本所用的颜色空间，副本的颜色可能看起来不同。

# draft
shrink_note_protected = 它受密码保护，而精简副本不会保留密码

# draft
shrink_note_put_back = 其缩小后的图像在副本中看起来不同，已恢复原样，因此副本并未变小

# draft
shrink_note_signed = 它已签名，精简副本会破坏其签名

# draft
shrink_note_signed_rights = 它带有其制作者签名的权限（可在 Adobe Reader 中填写并保存），精简副本会破坏这些权限

# draft
shrink_note_still_unlike = 即使恢复了其图像，所制作的副本看起来仍与它不同，因此未保留任何副本

# draft
shrink_note_unlike = 所制作的副本在图像以外的部分看起来与它不同，因此未保留任何副本

# draft
shrink_print = 打印

# draft
shrink_screen = 屏幕

# draft
shrink_small = 最小

# draft
sort_a_to_z = A 到 Z

# draft
sort_a_to_z_phrase = 从 A 到 Z

# draft
sort_changes_first = 已更改的在前

# draft
sort_changes_first_phrase = 已更改的在前

# draft
sort_changes_last = 已更改的在后

# draft
sort_changes_last_phrase = 已更改的在后

# draft
sort_highest = 从高到低

# draft
sort_highest_first = 从高到低

# draft
sort_largest = 从大到小

# draft
sort_largest_first = 从大到小

# draft
sort_longest = 从长到短

# draft
sort_longest_first = 从长到短

# draft
sort_lowest = 从低到高

# draft
sort_lowest_first = 从低到高

# draft
sort_newest = 从新到旧

# draft
sort_newest_first = 从新到旧

# draft
sort_oldest = 从旧到新

# draft
sort_oldest_first = 从旧到新

# draft
sort_phrase_accessed = 访问时间

# draft
sort_phrase_created = 创建时间

# draft
sort_phrase_deleted = 删除时间

# draft
sort_phrase_dimensions = 尺寸

# draft
sort_phrase_duration = 时长

# draft
sort_phrase_extension = 扩展名

# draft
sort_phrase_folder = 文件夹

# draft
sort_phrase_kind = 种类

# draft
sort_phrase_modified = 修改时间

# draft
sort_phrase_name = 名称

# draft
sort_phrase_owner = 所有者

# draft
sort_phrase_rating = 评分

# draft
sort_phrase_size = 大小

# draft
sort_phrase_tags = 标签

# draft
sort_phrase_taken = 拍摄时间

# draft
sort_phrase_type = 类型

# draft
sort_phrase_vcs = Git 状态

# draft
sort_shortest = 从短到长

# draft
sort_shortest_first = 从短到长

# draft
sort_smallest = 从小到大

# draft
sort_smallest_first = 从小到大

# draft
sort_title_accessed = 访问时间

# draft
sort_title_created = 创建时间

# draft
sort_title_deleted = 删除时间

# draft
sort_title_dimensions = 尺寸

# draft
sort_title_duration = 时长

# draft
sort_title_extension = 扩展名

# draft
sort_title_folder = 文件夹

# draft
sort_title_kind = 种类

# draft
sort_title_modified = 修改时间

# draft
sort_title_name = 名称

# draft
sort_title_owner = 所有者

# draft
sort_title_rating = 评分

# draft
sort_title_size = 大小

# draft
sort_title_tags = 标签

# draft
sort_title_taken = 拍摄时间

# draft
sort_title_type = 类型

# draft
sort_title_vcs = Git

# draft
sort_z_to_a = Z 到 A

# draft
sort_z_to_a_phrase = 从 Z 到 A

# draft
suggest_actions = 操作

# draft
suggest_complete = 补全

# draft
suggest_go = 转到

# draft
suggest_help = 此栏可输入的内容

# draft
suggest_history = 后退

# draft
suggest_reading = 您是不是要找

# draft
suggest_recent = 最近的搜索

# draft
suggest_saved = 已保存的搜索

# draft
suggest_search_in = 搜索范围

# draft
suggest_syntax = 搜索条件

# draft
suggest_tabs = 已打开的标签页

# draft
suggest_terms = 添加条件

# draft
sys_bad_folder_permissions = 目录权限无效

# draft
sys_group_not_yet = { $system } 上尚不支持更改文件的所属组

# draft
sys_no_proc = 系统的 /proc 未挂载，而更改访问权限需要它

# draft
sys_now_a_link = 它现在是链接

# draft
sys_now_another_file = 它现在是另一个文件

# draft
sys_path_not_utf8 = { $path } 不是 UTF-8 路径

# draft
sys_pipe_not_yet = { $system } 上尚不支持复制管道

# draft
sys_swap_windows = Windows 无法一步交换两个名称

# draft
sys_trash_list_not_yet = { $system } 上尚不支持列出回收站的内容

# draft
sys_trash_refused = 移到回收站 { $path }：{ $error }

# draft
sys_trash_restore_not_yet = { $system } 上尚不支持从回收站恢复

# draft
sys_unix_permissions_not_yet = { $system } 上尚不支持恢复 Unix 目录权限

# draft
sys_windows_permissions_not_yet = { $system } 上尚不支持恢复 Windows 目录权限

# draft
sys_write_through_not_yet = { $system } 上尚不支持将数据直写到驱动器

# draft
system_this = 此系统

# draft
tabs_close = 关闭标签页

# draft
tabs_close_description = 关闭一个标签页（默认为当前显示的标签页）；通知中提供“撤销”。若还有其他窗口打开，关闭窗口的最后一个标签页会关闭该窗口；最后一个窗口的最后一个标签页会保留（用该窗口自身的 Ctrl+W 可关闭窗口）

# draft
tabs_close_window = 关闭窗口

# draft
tabs_close_window_description = 关闭一个窗口（默认为活动窗口）及其标签页，“重新打开关闭的标签页”可将其恢复；最后一个窗口只能通过其自身的关闭按钮或“退出”关闭

# draft
tabs_closed = 已关闭标签页{ $title }

# draft
tabs_closed_undo = 撤销

# draft
tabs_go_to_family = 转到标签页

# draft
tabs_last = 最后一个标签页

# draft
tabs_last_tabs = 窗口上次关闭时显示的标签页（支持多窗口之前）

# draft
tabs_last_window = 最后一个窗口需通过其关闭按钮或“退出”关闭

# draft
tabs_last_windows = 上次显示的窗口及其标签页

# draft
tabs_list = 列出标签页

# draft
tabs_list_description = 按顺序逐个窗口列出标签页：每个标签页所在的窗口、其窗格、窗格的位置、哪个窗格处于活动状态，以及其窗口是否正在显示它

# draft
tabs_list_windows = 列出窗口

# draft
tabs_list_windows_description = 所有窗口：每个窗口的标签页、所显示的标签页，以及哪个窗口处于活动状态

# draft
tabs_move = 移动标签页

# draft
tabs_move_description = 将标签页放到另一个位置，或按步数移动；或将其移到另一个窗口或新窗口

# draft
tabs_move_family = 移动标签页

# draft
tabs_move_left = 将标签页左移

# draft
tabs_move_right = 将标签页右移

# draft
tabs_move_to_window = 将标签页移到新窗口

# draft
tabs_new = 新建标签页

# draft
tabs_new_description = 打开一个显示某个位置或窗格所示内容的标签页；除非 `show` 为 false，否则立即显示

# draft
tabs_new_window = 新建窗口

# draft
tabs_new_window_description = 打开一个显示某个位置或窗格所示内容的窗口，其形状与窗格所在的窗口相同；它会成为活动窗口

# draft
tabs_next = 下一个标签页

# draft
tabs_no_pane = 没有这个窗格

# draft
tabs_no_tab = 没有这个标签页

# draft
tabs_no_window = 没有这个窗口

# draft
tabs_nothing_for_tab = 窗格中没有可在标签页中打开的内容

# draft
tabs_nothing_for_window = 窗格中没有可在窗口中打开的内容

# draft
tabs_nth = 标签页 { $number }

# draft
tabs_only_folders = 只有文件夹和位置才能在标签页或窗口中打开

# draft
tabs_open = 在新标签页中打开

# draft
tabs_open_description = 将每个所选文件夹在各自的标签页中打开，置于当前标签页之后；文件不会在标签页中打开

# draft
tabs_open_window = 在新窗口中打开

# draft
tabs_open_window_description = 将每个所选文件夹在各自的窗口中打开，其形状与窗格所在的窗口相同；文件不会在窗口中打开

# draft
tabs_previous = 上一个标签页

# draft
tabs_reopen = 重新打开关闭的标签页

# draft
tabs_reopen_description = 重新打开最后关闭的标签页或窗口，或某次关闭所报告的那个，连同其历史记录

# draft
tabs_select = 显示标签页

# draft
tabs_select_description = 显示一个标签页：按 ID，或在活动窗口的标签页中按位置（负数从右侧算起）或从当前标签页起的步数

# draft
tabs_select_window = 转到窗口

# draft
tabs_select_window_description = 将一个窗口设为活动窗口并置于前台

# draft
tabs_sleep = 释放隐藏标签页的内存

# draft
tabs_sleep_description = 您未在查看的标签页会释放其文件列表，再次显示时重新列出。

# draft
tabs_sleep_description_more = 关闭此项可让每个标签页始终保持列出且为最新，但会占用更多内存和后台工作。

# draft
tabs_which_window = 哪个窗口：需要 `window`

# draft
tabs_window_one_tab = 该窗口只有一个标签页

# draft
topic_files = 文件

# draft
topic_find = 查找

# draft
topic_go = 导航

# draft
topic_notes = 标签和标注

# draft
topic_places = 位置和服务器

# draft
topic_player = 播放

# draft
topic_preview = 预览

# draft
topic_select = 选择

# draft
topic_skwir = Skwir

# draft
topic_tabs = 标签页和窗格

# draft
topic_terminal = 终端

# draft
topic_view = 视图和面板

# draft
topic_window = 窗口

# draft
trash_op_mounted_remote = 由程序挂载的网络文件夹没有回收站：请改为永久删除

# draft
trash_op_no_home = 找不到存放其回收站的主文件夹

# draft
trash_op_share_refused = 该网络共享没有您可使用的回收站：请改为永久删除

# draft
trash_op_top = 文件系统的顶层无法移到回收站

# draft
vcs_changed = 已更改

# draft
vcs_conflict = 有冲突

# draft
vcs_folder_ignored = 已忽略

# draft
vcs_holds_changes = 含有更改

# draft
vcs_holds_conflict = 含有冲突

# draft
vcs_holds_new = 含有新文件

# draft
vcs_holds_staged = 含有已暂存的更改

# draft
vcs_ignored = 已忽略

# draft
vcs_new = 新增

# draft
vcs_staged = 已暂存

# draft
volumes_drive_gone = 存放此文件夹的磁盘已被卸载或移除。磁盘挂载后，此文件夹会重新显示在这里。

# draft
volumes_server_gone = 存放此文件夹的服务器已断开连接。重新连接后，此文件夹会重新显示在这里。

# draft
workspace_no_tab = 没有这个标签页

# draft
workspace_no_window = 没有这个窗口

# draft
workspace_tab_alone = 该标签页是其窗口中唯一的标签页

# draft
workspace_tab_there = 该标签页已在那个窗口中
