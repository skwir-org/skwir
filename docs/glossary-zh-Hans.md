# Simplified Chinese glossary

Status: drafted 2026-11-18 by a model, **not yet reviewed by a speaker of
Chinese**. Every `zh-Hans.ftl` is drafted from it, each message marked
`# draft` until reviewed. A reviewer who changes a term here changes it in
the drafts by search, and says so in this file.

The terms follow what Simplified Chinese users already read in their file
managers (GNOME 文件, Dolphin, the Windows 文件资源管理器, the macOS 访达),
choosing the most common where they differ. Where an English word has two
senses (a pane and a panel, a tag and a tab, Skwir's notes and a
document's, a file's kind and a book's genre), each sense has its own
Chinese word: decide by the English of the same id and its note for
translators.

## Writing

- **No subject where none is needed.** An instruction says what to do
  (「选择一个文件夹」); where the reader must be named, 「您」, never 「你」.
- **Buttons and menu items are verbs:** 「复制」「重命名…」「清空回收站…」.
  What was done is said with 「已」: 「已将 3 个项目复制到 Documents」,
  「已重命名」.
- **A count takes its measure word:** 「3 个文件」「3 个文件夹」「3 个项目」
  「2 页」「5 首曲目」.
- **No full stop** where the English has none (a button, a title, a
  toast's answer); 「。」 where the English ends a sentence with one.
- **Ellipsis:** 「…」 (U+2026), once, after a command that asks something
  before it acts, as in English.

## Typography

- **Full-width punctuation** in Chinese text: 「，」「。」「：」「；」「！」
  「？」「（）」. Half-width only inside what stays as written (a path, a
  command, `tag:`, a file name).
- **Quotes:** “name” (U+201C, U+201D). Skwir adds them itself around a
  `Name` argument (its catalog's `name_quote_open` and
  `name_quote_close`): never write quotes around `{ $name }`. A `Folder`
  (where something goes, "to Docs") is not quoted.
- **A half-width space** between Chinese and Latin letters or digits,
  arguments included: 「3 个文件」「移到 { $folder }」「打开 PDF 文件」. None
  beside full-width punctuation or the quotes: 「已删除{ $name }。」 needs
  none, the quotes being Skwir's: a `Name` argument (a file's, a
  folder's, a place's or a tag's name, `docs/invariants/words.md`)
  takes no space on either side.
- **No-break spaces** stay where the English has them: a number and its
  unit (`{ $n }\u{a0}MB`) are never parted by a plain space (the catalogs'
  test refuses it). A percent follows its number: `{ $n }%`.
- **Numbers, sizes and dates** are formatted by Skwir (`Count`, `Size`,
  dates): write `{ $count }`, never a number. Units stay Latin: KB, MB, GB,
  TB; KiB, MiB, GiB, TiB.
- **Plurals:** Chinese has none (CLDR's only category is `other`). A
  selector on a count may go: 「{ $count } 个文件」. Keep a selector the
  English keeps for its meaning: `[1]` where one named thing is said apart
  from several counted, and every selector on anything but a count
  (`$kind`, `$inside`, `$file`).

## Places in sentences

Skwir's places are nouns (`# noun` above them in `en.ftl`). Chinese needs
neither gender nor forms: a place is written as it is named, with no
attribute (`tools/words/grammar` has no `zh-Hans` line).

| Place | Chinese |
| --- | --- |
| Home | 主文件夹 |
| Trash | 回收站 |
| Recent | 最近使用 |
| Shelf | 暂存架 |
| Nearby | 附近 |
| Root (the system's volume) | 根目录 |

## Terms

| English | Chinese | Note |
| --- | --- | --- |
| file | 文件 | |
| folder | 文件夹 | |
| item (file or folder) | 项目 | 「3 个项目」 |
| entry | 项目 | an entry in a listing |
| link (symbolic) | 链接 | 「符号链接」 where it must be said whole |
| Trash | 回收站 | a place |
| move to the trash | 移到回收站 | |
| empty the trash | 清空回收站 | |
| restore | 恢复 | |
| delete (for good) | （永久）删除 | |
| copy / cut / paste | 复制 / 剪切 / 粘贴 | |
| move | 移动 | |
| rename | 重命名 | |
| undo / redo | 撤销 / 重做 | |
| new folder | 新建文件夹 | |
| open / open with | 打开 / 打开方式 | 「用 { $app } 打开」 |
| hidden files | 隐藏文件 | |
| tag | 标签 | the query language's `tag:` stays as written; TagSpaces' own words stay |
| tab | 标签页 | never 标签, which is a tag |
| rating | 评分 | 「4 星」; 「未评分」 |
| comment | 注释 | |
| notes (tags, rating, comment) | 标注 | Skwir's word for the three together |
| a document's own notes (speaker notes, footnotes) | 备注 | 「演讲者备注」, 「脚注」 |
| Properties | 属性 | |
| preview | 预览 | |
| quick preview | 快速预览 | |
| thumbnail | 缩略图 | |
| sidebar | 侧边栏 | |
| place (sidebar) | 位置 | |
| location | 位置 | |
| address (field) | 地址 | 「地址栏」 for the field |
| trail (the path's crumbs) | 路径栏 | |
| pane | 窗格 | one of the panes showing folders, side by side when split |
| panel (the preview's, the terminal's, a side panel) | 面板 | 「预览面板」「终端面板」; never 窗格 |
| split | 分屏 | |
| view: list, grid, columns, board | 视图：列表、网格、分栏、看板 | |
| sort by / group by | 排序方式 / 分组方式 | 「按名称排序」 |
| folders first | 文件夹优先 | |
| name, size, modified, created, accessed | 名称、大小、修改时间、创建时间、访问时间 | |
| type (a file's) | 类型 | its type name, the MIME type's description |
| kind (what a type is: image, video, document…) | 种类 | `kind:` stays as written |
| a book's literary genre | 体裁 | never 种类 |
| search / filter | 搜索 / 筛选 | |
| saved search | 已保存的搜索 | |
| Quick filters | 快速筛选 | |
| Shelf | 暂存架 | Skwir's place for what waits to be pasted |
| bookmark | 书签 | |
| Recent | 最近使用 | |
| Home | 主文件夹 | |
| Nearby | 附近 | |
| remote folder / server | 远程文件夹 / 服务器 | |
| connect / disconnect | 连接 / 断开连接 | |
| mount / unmount, eject | 挂载 / 卸载，弹出 | |
| device, volume, drive, disk | 设备、卷、驱动器、磁盘 | |
| plan (changes Skwir proposes) | 计划 | the changes reviewed before they apply |
| proposal / review | 提议 / 审阅 | |
| apply | 应用 | |
| step | 步骤 | |
| Activity | 活动 | |
| History | 历史记录 | |
| Earlier | 较早 | Activity's section of what ended |
| cancel / stop | 取消 / 停止 | |
| conflict, name taken | 冲突，名称已被占用 | |
| replace / keep both / skip | 替换 / 保留两者 / 跳过 | |
| merge (folders) | 合并 | |
| command palette | 命令面板 | |
| shortcut, key | 快捷键，按键 | |
| Settings | 设置 | |
| exclusions | 排除项 | |
| plugin | 插件 | another program's own plugins keep the words their program gives them |
| plugin from outside (ADR-045) | 外部插件 | |
| WebAssembly module | WebAssembly 模块 | |
| agent | 智能体 | an AI or a script that drives Skwir |
| Skwir (the product, in prose) | Skwir | 「skwir」 where it names the program a terminal runs, a path or an id, and where the product shows its name as a brand: the window's title and the launcher's name, lowercase by the maintainer's choice |
| Usage (disk usage view) | 磁盘占用 | |
| extract / compress | 解压 / 压缩 | |
| archive | 压缩文件 | |
| Make Smaller Copy | 制作精简副本 | |
| print / save as PDF | 打印 / 另存为 PDF | |
| page, booklet | 页，小册子 | |
| player, track, playlist | 播放器，曲目，播放列表 | |
| volume (sound) | 音量 | |
| fullscreen | 全屏 | |
| zoom in / out | 放大 / 缩小 | |
| terminal | 终端 | |
| map | 地图 | |
| Ctrl, Shift, Alt, Super, Esc, Enter, Tab, Space, Backspace, Delete | Ctrl, Shift, Alt, Super, Esc, Enter, Tab, 空格, Backspace, Delete | as printed on the keys; Space as GNOME writes it |
| Page Up / Page Down, Home / End | Page Up / Page Down, Home / End | |
| this language's name in itself | 简体中文 | |

## Terms the drafts agreed on

Recurring words the drafts settled among themselves on 2026-11-18, which
the table above did not list.

| English | Chinese | Note |
| --- | --- | --- |
| lens (a pane's facets, ADR-107) | 透镜 | |
| pill (a search's or a filter's) | 胶囊 | |
| footer; Activity's strip | 底栏；活动条 | |
| fold / unfold | 折叠 / 展开 | |
| waiting its turn | 排队中 | |
| Peek | 速览 | |
| companion files; sidecar | 伴随文件；附属文件 | |
| folder opened in place | 就地展开的文件夹 | |
| Rename several, bulk rename | 批量重命名 | its fields: 模板, 正则表达式, 替换为, 起始值 |
| record (a settings list's row) | 条目 | |
| Undo history | 撤销历史记录 | |
| Changed by you | 由您更改 | |
| context menu | 右键菜单 | |
| turn a plugin on / off | 启用 / 停用 | |
| Automatic, Light, Dark, Default (choices) | 自动、浅色、深色、默认 | |
| sort directions | 从新到旧、从旧到新、从大到小、从小到大、从长到短、从短到长、从高到低、从低到高、A 到 Z | |
| access (permissions); owner, group | 访问权限；所有者、所属组 | |
| keyring; passphrase | 密钥环；密码短语 | |
| provider (software); provider (a storage company) | 提供程序；提供商 | |
| helper program | 辅助程序 | |
| dry run | 试运行 | |
| Agents and scripts | 智能体与脚本 | |
| smaller copy; Reduce Size…; the qualities | 精简副本；减小体积…；无损、打印、屏幕、最小 | |
| Source (a preview's way of looking) | 源代码 | |
| Still frames (the video engine's choice) | 静止画面 | |
| timeline; its bar | 时间线；柱条 | |
| map place; track; map tiles | 地点；轨迹；地图瓦片 | |
| geotag | 地理标记 | kept apart from 标签 |
| a document's comments | 批注 | not 注释, a file's comment |
| member (of an archive or a package) | 成员 | |

## For the reviewer

Each draft's makers listed what they were least sure of; these are the
places to read first.

- **Destructive actions** (the worst place for a wrong word): every
  `deleting_*` and `prompt_delete` (ui), the `remote_*_remove_*` intros
  (remote), `trash_op_*` (core), `toast_quit_how` (撤销 for what Quit
  takes back: also Undo's word).
- **Sentences built from parts:** `query_reading` and its parts (each a
  clause after 「，」), `geo_left_*` with `geo_left_out`, `access_clause_*`
  with `access_does_*` (chmod's capital X), `remote_servers_kept_*` (written
  as 「在…中」 to fit every sentence that places them).
- **Spacing that depends on the argument:** `system_this` (此系统) leaves a
  stray space in 「{ $system } 上尚不支持…」; `query_named_in`,
  `exclusions_said_beside`, `properties_give_to_group`, `toast_deleted_kept`,
  `address_in`, `address_above` and `address_folders_above` (a place of
  Skwir's, 主文件夹, or a folder's own name): an argument that is
  sometimes Latin, sometimes Chinese.
- **Hard to say briefly:** `omnibox_as_you_type` (即时), `keymap_not`
  (除{ $condition }以外), `properties_changes` and `properties_can_run`,
  `clashes_folders_onto_folders_for_all`, the Exclusions page's roles
  (对智能体隐藏, 排除于), `status_as_of_index`, `usage_changed_within`,
  `jump_plugin_description` (frecency), `media_replay_gain_description`.
- **Words to confirm:** 目录 for a document's contents (also "directory"),
  脚注 for a note (or an endnote), 提供者 for a place's owner
  (`sidebar_kept_by_owner`), 回滚历史 for a terminal's scrollback, 一个智能体
  for "an agent", Legal and Letter kept as written for the paper sizes.
- **Samples:** `settings_specimen`, `fonts_sample` and `print_font_sample`
  have no pangram in Chinese: a line of the 千字文 or Pango's sample
  sentence, then digits.

