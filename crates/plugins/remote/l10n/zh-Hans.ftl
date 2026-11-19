# Simplified Chinese catalog of skwir-plugin-remote, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
remote_address_bad_device = “{ $text }”不是设备名称

# draft
remote_address_bad_port = “{ $port }”不是有效的端口

# draft
remote_address_bad_remote = “{ $name }”不是有效的远程名称

# draft
remote_address_bad_server = “{ $host }”不是有效的服务器名称

# draft
remote_address_bad_ssh_user = “{ $user }”不是可以交给 ssh 的用户名

# draft
remote_address_bad_user = “{ $user }”不是有效的用户名

# draft
remote_address_device_user = “{ $text }”：设备的地址不包含用户

# draft
remote_address_ipv6 = “{ $text }”缺少结束其 IPv6 地址的 ]

# draft
remote_address_no_protocol = “{ $text }”未指定协议

# draft
remote_address_no_server = “{ $text }”未指定服务器

# draft
remote_address_no_slashes = “{ $text }”在服务器之前需要 //

# draft
remote_address_not_protocol = “{ $scheme }:”不是 Skwir 能打开的协议

# draft
remote_address_password = 地址中不包含密码：服务器会询问密码

# draft
remote_ask_connect = 连接

# draft
remote_ask_password = 密码

# draft
remote_bucket_label = { $bucket } ({ $profile })

# draft
remote_cancelled = 已取消

# draft
remote_chosen_data = 为每个远程文件夹选择的缩略图设置，按其地址

# draft
remote_configure_add_as_server = 添加为服务器…

# draft
remote_configure_add_record = 添加服务器

# draft
remote_configure_add_server = 添加服务器…

# draft
remote_configure_add_server_description = 添加服务器：一个 rclone 远程（云存储、S3、WebDAV、SFTP 等七十多种），通过用户填写的表单添加；可以给出表单的初始内容，但绝不包括机密

# draft
remote_configure_add_server_menu = 添加服务器…

# draft
remote_configure_added = 已添加{ $server }

# draft
remote_configure_added_obscured = 已添加{ $server }。rclone 仅以混淆形式保存其机密：要加密其配置吗？

# draft
remote_configure_answers = { $server }响应正常

# draft
remote_configure_at_login = 登录时连接

# draft
remote_configure_at_login_encrypted = { $remotes }会在登录时连接，而加密的配置无法做到这一点：登录时没有人能提供其密码。请先关闭“登录时连接”。

# draft
remote_configure_at_login_failed = 未能将{ $server }设为登录时连接：{ $error }

# draft
remote_configure_at_login_on = 登录时连接 ✓

# draft
remote_configure_at_login_which = 只有 rclone 远程和 SFTP 服务器可以在登录时连接

# draft
remote_configure_bookmark_stays = 其书签仍然保留：{ $error }

# draft
remote_configure_connect_at_login = 登录时连接

# draft
remote_configure_connect_at_login_description = 让系统的服务管理器在用户登录时连接某个服务器（systemd 用户单元、启动代理、登录任务），或不再这样做；同时立即连接。仅限登录时无需询问任何内容的服务器：配置未加密的 rclone 远程，或通过密钥登录的 SFTP 服务器

# draft
remote_configure_connects_at_login = { $server }将在登录时连接

# draft
remote_configure_copied_what = 公开链接

# draft
remote_configure_copy_public_link = 复制公开链接

# draft
remote_configure_does_not_answer = { $server }没有响应：{ $error }

# draft
remote_configure_edit_server = 编辑服务器…

# draft
remote_configure_edit_server_description = 在表单中更改服务器：rclone 远程，或通过地址添加的服务器

# draft
remote_configure_edit_server_menu = 编辑服务器…

# draft
remote_configure_encrypt = 加密 rclone 的配置…

# draft
remote_configure_encrypt_accept = 加密

# draft
remote_configure_encrypt_again = 再次输入

# draft
remote_configure_encrypt_button = 加密…

# draft
remote_configure_encrypt_description = 用用户输入的密码加密 rclone 的配置文件（其中远程的机密仅以混淆形式保存），该密码可以保存在密钥环中

# draft
remote_configure_encrypt_intro = rclone 将其远程的密钥和密码保存在配置文件中，仅作了混淆：任何能读取该文件的人都能使用它们。加密后，需要密码才能打开。

# draft
remote_configure_encrypt_keep = 保存在密钥环中

# draft
remote_configure_encrypt_keep_help = 这样 Skwir 打开配置时就无需询问。在终端中运行的 rclone 会询问密码，或从 RCLONE_CONFIG_PASS 中读取。

# draft
remote_configure_encrypt_password = 密码

# draft
remote_configure_encrypt_title = 加密 rclone 的配置

# draft
remote_configure_encrypted = rclone 的配置已加密

# draft
remote_configure_encrypted_already = rclone 的配置早已加密

# draft
remote_configure_finish = 完成设置…

# draft
remote_configure_forget_password = 忘记密码

# draft
remote_configure_keep_as_server = 保留为服务器

# draft
remote_configure_keep_as_server_description = 将远程文件夹的地址作为“网络”部分的服务器，以其书签的名称保存在 Skwir 的列表或 KDE 的网络文件夹中；如有书签，则移除该书签

# draft
remote_configure_keep_as_server_item = 保留为服务器

# draft
remote_configure_keep_as_server_menu = 在位置的菜单中提供“保留为服务器”

# draft
remote_configure_keep_as_server_menu_description = 侧边栏中位置的“保留为服务器”，用于尚未成为服务器的远程文件夹地址（书签的地址）。从内存中作答

# draft
remote_configure_kept_as_server = { $server }现已成为“网络”部分的服务器，保存{ $kept }

# draft
remote_configure_kept_by_tools = { $protocol } 由 rclone 或 AWS 工具保存

# draft
remote_configure_kept_elsewhere = { $server }{ $kept }：请在那里修改

# draft
remote_configure_kept_stays = Skwir 为{ $server }保存的内容仍然保留：{ $error }

# draft
remote_configure_key_login = 设置密钥登录…

# draft
remote_configure_key_login_failed = 未能在{ $server }上设置密钥登录：{ $error }

# draft
remote_configure_key_login_sftp = 密钥登录属于 ssh：仅限 SFTP 服务器

# draft
remote_configure_mount_options = 挂载选项…

# draft
remote_configure_mount_options_button = 挂载选项…

# draft
remote_configure_mount_options_description = 选择服务器下次连接时的挂载方式：只读、rclone 的文件缓存和带宽、sshfs 的压缩

# draft
remote_configure_moved_off_login = { $server }不再在登录时连接：其地址已更改

# draft
remote_configure_nearby_menu = 附近服务器的菜单

# draft
remote_configure_nearby_menu_description = 在“附近”位置中选中单个服务器时，右键菜单中的项目：“添加为服务器…”，其表单已填入该服务器所通告的名称和地址（前提是没有服务器使用该地址）。从内存中作答

# draft
remote_configure_no_longer_at_login = { $server }不再在登录时连接

# draft
remote_configure_no_options = Skwir 用于挂载 { $protocol } 的工具都不接受选项

# draft
remote_configure_no_public_link = 未生成公开链接：{ $error }

# draft
remote_configure_not_done = { $done ->
        [added] 未添加服务器：{ $error }
        [saved] 未保存服务器：{ $error }
        [signed_in] 登录服务器失败：{ $error }
        [tested] 未测试服务器：{ $error }
        [kept] 未保留服务器：{ $error }
       *[removed] 未移除服务器：{ $error }
    }

# draft
remote_configure_not_encrypted = rclone 的配置未加密：{ $error }

# draft
remote_configure_not_remote = 它不在远程文件夹中

# draft
remote_configure_nothing_to_link = 没有可链接的内容

# draft
remote_configure_options_not_at_login = 挂载选项已保存，但登录时连接仍按原来的方式挂载：{ $error }

# draft
remote_configure_options_saved = { $server }将在下次连接时按此挂载

# draft
remote_configure_options_unsaved = 挂载选项未保存：{ $error }

# draft
remote_configure_password_stays = Skwir 为{ $server }保存的密码仍在密钥环中：{ $error }

# draft
remote_configure_passwords_differ = 两次输入的密码不一致

# draft
remote_configure_public_link = 复制公开链接

# draft
remote_configure_public_link_description = 为存储支持公开链接的远程中的文件生成公开链接（云盘的共享链接），并将其放到剪贴板上

# draft
remote_configure_public_link_menu = 在右键菜单中提供公开链接

# draft
remote_configure_public_link_menu_description = 右键菜单中的“复制公开链接”，用于存储支持公开链接的远程中的文件。从内存中作答

# draft
remote_configure_reconnect = 重新登录…

# draft
remote_configure_reconnect_description = 在 rclone 远程的访问权限过期或被撤销后，通过浏览器重新登录它

# draft
remote_configure_record_kept = { $kind } · { $kept }

# draft
remote_configure_record_kind = 种类

# draft
remote_configure_record_name = 名称

# draft
remote_configure_record_unfinished = { $kind } · { $kept } · 尚未完成设置

# draft
remote_configure_remove = 移除

# draft
remote_configure_remove_intro = Skwir 将忘记此服务器（保存{ $kept }）：服务器上的内容仍保留在原处，重新添加即可恢复。

# draft
remote_configure_remove_intro_password = Skwir 将忘记此服务器（保存{ $kept }）以及为其保存在密钥环中的密码：服务器上的内容仍保留在原处，重新添加即可恢复。

# draft
remote_configure_remove_server = 移除服务器…

# draft
remote_configure_remove_server_description = 在用户确认后，从服务器的保存位置（rclone 的配置、Skwir 的列表、KDE 的网络文件夹）移除该服务器，连同其挂载选项及登录时连接它的条目：服务器上的内容仍保留在原处

# draft
remote_configure_remove_server_menu = 移除服务器…

# draft
remote_configure_remove_title = 要移除{ $server }吗？

# draft
remote_configure_removed = 已移除{ $server }

# draft
remote_configure_saved = 已保存{ $server }

# draft
remote_configure_server_records = 服务器

# draft
remote_configure_server_records_description = 设置中列出的每个服务器：各自的名称、种类和保存位置、其所在行的作用，以及 Skwir 是否会更改它（`fixed`：由另一个程序更改）

# draft
remote_configure_servers = 服务器

# draft
remote_configure_servers_description = 侧边栏“网络”部分中的服务器，各自在其保存位置更改。

# draft
remote_configure_servers_description_more = 通过地址添加的服务器保存在 Skwir 的列表或 KDE 的网络文件夹中；rclone 远程保存在 rclone 自己的配置中，在终端中运行的 rclone 也能找到它们。

# draft
remote_configure_setup_key_login = 设置密钥登录…

# draft
remote_configure_setup_key_login_description = 让 ssh 通过密钥而非密码登录 SFTP 服务器：使用 ssh 代理持有的密钥、用户的默认密钥或一个新密钥，用户选定后将其公钥添加到服务器的授权密钥中

# draft
remote_configure_sign_in_again = 重新登录…

# draft
remote_configure_signed_in = 已重新登录{ $server }

# draft
remote_configure_stays_mounted = 它仍处于挂载状态：{ $error }

# draft
remote_configure_test = 测试

# draft
remote_configure_test_server = 测试服务器

# draft
remote_configure_test_server_description = 像连接时那样列出 rclone 远程的顶层，并说明它是否有响应

# draft
remote_configure_test_server_menu = 测试服务器

# draft
remote_configure_turned_off = 远程文件夹已停用

# draft
remote_configure_unknown_server = { $address } 处没有已知的服务器

# draft
remote_configure_window_needed = 这需要在 Skwir 窗口的表单中填写：请先打开窗口

# draft
remote_connect = 连接到服务器

# draft
remote_connect_description = 通过地址打开远程文件夹（sftp://、smb://、dav://、ftp://、nfs://、afp://、rclone://）

# draft
remote_connectors = 打开地址的工具

# draft
remote_connectors_description = 挂载 sftp://、smb:// 或其他地址的工具，按此顺序在已安装的工具中尝试：sshfs、kmtp（KDE 持有的手机）、gvfs、kio、rclone、fstab。

# draft
remote_connectors_description_more = 在 KDE 桌面上，kio 排在 gvfs 之前。

# draft
remote_device_held = { $programs } 正在使用它，而{ $device ->
        [phone] 手机
       *[camera] 相机
    }一次只能由一个程序使用。{ $count ->
        [1] 请退出 { $programs }，然后重试。
       *[other] 请退出这些程序，然后重试。
    }

# draft
remote_device_held_through = { $programs } 正在使用它，而{ $device ->
        [phone] 手机
       *[camera] 相机
    }一次只能由一个程序使用。Skwir 本可以通过 { $holder } 打开它，但{ $why }；否则{ $count ->
        [1] 请退出 { $programs }，然后重试。
       *[other] 请退出这些程序，然后重试。
    }

# draft
remote_device_not_mounted = 未挂载。点击以挂载并打开。

# draft
remote_disconnect = 断开连接

# draft
remote_disconnect_description = 断开远程文件夹：正在显示它的窗格将返回主文件夹

# draft
remote_disconnected = 已断开与{ $name }的连接

# draft
remote_disconnected_kept = 已断开与{ $name }的连接：{ $count } 个尚未发送的文件保留在 rclone 的缓存中，将在下次连接时发送

# draft
remote_disconnecting = 正在断开连接…

# draft
remote_forget_password = 忘记密码

# draft
remote_forget_password_description = 从密钥环中移除 Skwir 为某个服务器保存的密码（绝不移除其他程序保存的密码）

# draft
remote_forget_password_menu = 忘记密码

# draft
remote_fstab = 系统挂载

# draft
remote_fstab_administrator = 它由系统挂载：只有管理员才能卸载它

# draft
remote_fstab_linux_only = 仅在 Linux 上读取 fstab

# draft
remote_fstab_no_entry = /etc/fstab 中没有指向它且您可以挂载的条目

# draft
remote_gvfs_camera_label = { $device }（相机）

# draft
remote_gvfs_connect_to = 要连接到 { $host } 吗？

# draft
remote_gvfs_device_silent = 它在 { $seconds } 秒内没有响应

# draft
remote_gvfs_ios_label = { $device } (iOS)

# draft
remote_gvfs_linux_only = GVfs 仅在 Linux 上运行

# draft
remote_gvfs_mtp_label = { $device } (MTP)

# draft
remote_gvfs_no_address = GVfs 未给出此挂载的地址

# draft
remote_gvfs_no_bridge = GVfs 已挂载 { $address }，但其 FUSE 桥接未运行：是否已安装 gvfsd-fuse？

# draft
remote_gvfs_no_folder = GVfs 已挂载 { $address }，但其文件夹不存在

# draft
remote_gvfs_other_label = { $host } ({ $kind })

# draft
remote_gvfs_server_silent = { $host } 在 { $seconds } 秒内没有响应

# draft
remote_gvfs_you_cancelled = 您已取消

# draft
remote_half_made = { $name } 尚未完成设置：请在其菜单中选择“完成设置…”

# draft
remote_keys_already_there = { $path } 处已有密钥

# draft
remote_keys_comment_line = 密钥的注释只能有一行

# draft
remote_keys_form_accept = 设置

# draft
remote_keys_form_again = 再次输入

# draft
remote_keys_form_intro = 您的密钥的公钥部分将添加到服务器的授权密钥中：之后 ssh 将用它登录，无需密码。其私钥部分绝不会离开本机。服务器的密码只会询问一次。

# draft
remote_keys_form_key = 密钥

# draft
remote_keys_form_passphrase = 新密钥的密码短语

# draft
remote_keys_form_passphrase_help = 用于加密新密钥：使用密钥时会询问，除非代理持有它。留空表示不设置。

# draft
remote_keys_form_title = 在{ $server }上设置密钥登录

# draft
remote_keys_had_it = { $server }已有此密钥：ssh 会用它登录

# draft
remote_keys_in_agent = { $key }，位于 ssh 代理中

# draft
remote_keys_in_file = { $key }，位于 { $file }

# draft
remote_keys_key = { $comment } ({ $kind })

# draft
remote_keys_keygen_failed = ssh-keygen 失败：{ $said }

# draft
remote_keys_keygen_no_passphrase = 未向 ssh-keygen 提供密码短语

# draft
remote_keys_keygen_no_public = ssh-keygen 未生成公钥

# draft
remote_keys_new = 新密钥

# draft
remote_keys_new_made_in = 创建于 { $folder }

# draft
remote_keys_no_home = 没有用于存放 ssh 密钥的主文件夹

# draft
remote_keys_no_runtime = 没有运行时文件夹

# draft
remote_keys_no_such_key = 没有这个密钥

# draft
remote_keys_none_chosen = 未选择密钥

# draft
remote_keys_none_offered = ssh 未提供任何密钥，而新建的密钥会覆盖 { $path } 处的密钥

# draft
remote_keys_not_added = 服务器无法添加密钥：{ $why }

# draft
remote_keys_not_added_unsaid = 服务器无法添加密钥：它未说明原因

# draft
remote_keys_not_taken = 密钥已在{ $server }上，但 ssh 无法用它登录：该服务器可能不接受密钥

# draft
remote_keys_passphrases_differ = 两次输入的密码短语不一致

# draft
remote_keys_refused = 服务器拒绝了密钥：{ $said }

# draft
remote_keys_set_up = 已在{ $server }上设置密钥登录

# draft
remote_keys_sftp_only = 密钥登录仅适用于 SFTP 服务器

# draft
remote_keys_unread = 无法读取服务器的授权密钥：{ $said }

# draft
remote_kio_could_not = KIO 无法打开它：{ $reason }

# draft
remote_kio_could_not_open = KIO 无法打开它

# draft
remote_kio_keeps = KIO 会保留其文件夹，直到会话结束

# draft
remote_kio_linux_only = KIO 的文件夹（kio-fuse）仅在 Linux 上可用

# draft
remote_kio_no_answer = kio-fuse 没有响应

# draft
remote_kio_no_folder = kio-fuse 已挂载它，但其文件夹不存在

# draft
remote_kio_opens_no = KIO 无法打开 { $protocol } 地址

# draft
remote_kmtp_not_held = KDE 的 kmtpd 未持有这部手机

# draft
remote_kmtpd = KDE 的 kmtpd

# draft
remote_kmtpd_linux_only = KDE 的 kmtpd 仅能在 Linux 上访问

# draft
remote_login_agent_read_only = 登录代理是只读的；请在管理它的地方修改

# draft
remote_login_control_character = “{ $part }”包含服务管理器无法读取的字符

# draft
remote_login_ended = 它已结束（{ $status }）

# draft
remote_login_journal = { $error }（journalctl --user -u { $unit } 会说明原因）

# draft
remote_login_needs_password = 服务器需要密码或密钥的密码短语：登录时没有人能提供（ssh 代理持有的密钥，或没有密码短语的密钥，可以自行登录）

# draft
remote_login_no_config = 没有配置文件夹

# draft
remote_login_no_folder = 没有可用于挂载的文件夹

# draft
remote_login_no_home = 没有主文件夹

# draft
remote_login_no_runtime = 没有可用于挂载的运行时文件夹

# draft
remote_login_no_schtasks = 缺少任务计划程序的 schtasks

# draft
remote_login_no_systemd = 没有可用于登录时连接的 systemd（没有 systemctl）

# draft
remote_login_no_user = Windows 未说明当前用户是谁（USERNAME）

# draft
remote_login_not_reached = 无法连接到它以确认它能否自行登录：{ $why }

# draft
remote_login_not_text = “{ $part }”不是服务管理器能读取的文本

# draft
remote_login_not_yet = { $system } 暂不支持登录时连接

# draft
remote_login_rclone_encrypted = rclone 的配置已加密：登录时没有人能提供其密码

# draft
remote_login_rclone_no_remote = rclone 的配置中没有远程{ $remote }

# draft
remote_login_rclone_not_yet = { $system } 暂不支持挂载 rclone 远程

# draft
remote_login_rclone_remotes_only = 只有 rclone 配置中的远程（rclone://name/）可以通过 rclone 在登录时连接：登录时没有人能提供服务器的密码

# draft
remote_login_sshfs_sftp_only = sshfs 只能连接 SFTP 地址

# draft
remote_login_unit_read_only = 登录单元是只读的；请在管理它的地方修改

# draft
remote_login_unknown_host = ssh 尚不知道该服务器的身份：请先连接它一次

# draft
remote_lost = 连接已断开：打开它以重新连接

# draft
remote_macfuse_missing = 未安装 macFUSE 或 FUSE-T

# draft
remote_mount_options_menu = 挂载选项…

# draft
remote_mounter_connected_already = 它已连接

# draft
remote_mounter_ended = 它已结束（{ $status }）

# draft
remote_mounter_no_runtime_folder = 没有可用于挂载的运行时文件夹

# draft
remote_mounter_no_runtime_folder_at_all = 没有运行时文件夹

# draft
remote_mounter_nothing_mounted = 它说已挂载，但那里没有挂载任何内容

# draft
remote_mounter_trust = 要连接到此服务器吗？

# draft
remote_mounter_used_by = { $programs }正在使用它

# draft
remote_mounter_used_by_another = 另一个程序正在使用它

# draft
remote_mounter_you_cancelled = 您已取消

# draft
remote_nearby_browser_failed = { $finder }：{ $error }

# draft
remote_nearby_empty = 附近未找到服务器

# draft
remote_nearby_empty_detail = 在此网络上通告自身的服务器（NAS、Mac、Windows 电脑）会显示在这里，在显示此位置时由 Avahi 和 wsdd 发现。

# draft
remote_nearby_failures = { $first }；{ $then }

# draft
remote_nearby_no_runtime = 没有运行时文件夹

# draft
remote_nearby_none_answers = 附近的服务器由 Avahi 或 wsdd 发现，但此处两者都没有响应（{ $why }）

# draft
remote_nearby_not_yet = { $system } 暂不支持查找附近的服务器

# draft
remote_nearby_server_kind = { $protocol } 服务器

# draft
remote_nearby_title = 附近

# draft
remote_nearby_wsdd_ended = wsdd 一启动就结束了（{ $status }）：其版本可能太旧，无法进行发现

# draft
remote_nearby_wsdd_no_start = wsdd 未启动

# draft
remote_needs_gio = 通过 GVfs（桌面的远程功能）打开地址

# draft
remote_needs_kmtp = 打开由 KDE 的 kmtpd 持有的手机

# draft
remote_needs_rclone = 打开 rclone 远程及其他地址

# draft
remote_needs_sshfs = 通过您的 ssh 打开 SFTP 地址

# draft
remote_no_password_kept = Skwir 没有为 { $server } 保存密码

# draft
remote_no_tool = Skwir 所知的工具都无法打开 { $protocol } 地址

# draft
remote_not_disconnected = 未能断开与{ $name }的连接：{ $error }

# draft
remote_not_installed = 未安装 { $program }

# draft
remote_not_mounted_there = 那里没有挂载远程文件夹

# draft
remote_nothing_installed = 已安装的工具都无法打开它：{ $why }

# draft
remote_options_bandwidth = 带宽限制

# draft
remote_options_bandwidth_help = rclone 的写法：10M，或以 10M:1M 分别指定上传和下载；留空表示不限制

# draft
remote_options_bandwidth_refused = 带宽是一个速率，如 10M 或 512k，或分别指定上传和下载，如 10M:1M

# draft
remote_options_cache = 文件缓存

# draft
remote_options_cache_automatic = 自动

# draft
remote_options_cache_automatic_detail = 此处使用 { $cache }：{ $why ->
        [wraps] 它加密或封装其他远程，若使用 full，这些远程的文件会保存在此磁盘上
        [near] 它位于本地网络上
       *[far] 读取过的内容会再次从此磁盘读取
    }

# draft
remote_options_cache_full = 读取和写入的内容在此磁盘上保留一小时：再次读取时从此处读取，关闭后发送，失败时重试

# draft
remote_options_cache_help = rclone 在此磁盘上保留其文件的哪些内容

# draft
remote_options_cache_minimal = 每次读取都会询问服务器；完整写入的文件在关闭后即位于服务器上

# draft
remote_options_cache_off = 不在此磁盘上保留任何内容：某些程序将无法写入

# draft
remote_options_cache_writes = 写入的文件会保留在此磁盘上，关闭后发送；每次读取都会询问服务器

# draft
remote_options_compression = 压缩

# draft
remote_options_compression_help = ssh 会压缩其发送的内容（sshfs）：在慢速网络上更快，在快速网络上更慢

# draft
remote_options_data = 为每个服务器选择的挂载选项，按其地址

# draft
remote_options_intro = 在下次连接时由 rclone 或 sshfs 使用。GVfs 和 KIO 按桌面的方式挂载。

# draft
remote_options_read_only = 只读

# draft
remote_options_read_only_help = 任何程序都不会在那里写入内容

# draft
remote_options_save = 保存

# draft
remote_options_title = { $server }的挂载选项

# draft
remote_password_for = { $label } 的密码

# draft
remote_password_forgotten = 已忘记密码

# draft
remote_password_not_forgotten = 未能忘记密码：{ $error }

# draft
remote_password_not_kept = 未能记住密码：{ $why }

# draft
remote_plugin_description = 通过 sshfs、GVfs、rclone 或系统挂载，将 SFTP、SMB、WebDAV、FTP、NFS 和 rclone 地址作为文件夹打开；通过 GVfs 打开手机和相机

# draft
remote_plugin_title = 远程文件夹

# draft
remote_provider_agent_connects = 一个智能体请求连接到 { $host }

# draft
remote_provider_agent_connects_detail = 它请求打开 { $address }。连接会让服务器知道您是谁。

# draft
remote_provider_agent_opens = 一个智能体请求打开 { $label }

# draft
remote_provider_agent_opens_detail = 它请求打开 { $address }。

# draft
remote_provider_camera_on = 相机开机后才会显示其文件。

# draft
remote_provider_connecting = 正在连接到 { $label }…

# draft
remote_provider_not_allowed = 您未允许

# draft
remote_provider_not_connected = 无法连接到 { $label }：{ $error }

# draft
remote_provider_not_opened = 无法打开 { $label }：{ $error }

# draft
remote_provider_opening = 正在打开 { $label }…

# draft
remote_provider_phone_unlocked = 手机解锁后才会显示其文件。

# draft
remote_provider_quitting = Skwir 正在退出

# draft
remote_provider_window_asks = 如果服务器要求输入密码，或首次连接时要求信任它，窗口会询问您。

# draft
remote_rclone_add = 添加

# draft
remote_rclone_add_kind = 添加 { $kind }

# draft
remote_rclone_add_server = 添加服务器

# draft
remote_rclone_add_server_intro = 通过地址添加的服务器，保存在 Skwir 的列表或 KDE 的网络文件夹中；或者 rclone 远程，保存在 rclone 自己的配置中，在终端中运行的 rclone 也能找到它。

# draft
remote_rclone_answer = 回答

# draft
remote_rclone_config_open = 打开

# draft
remote_rclone_config_password = rclone 配置的密码

# draft
remote_rclone_config_unsaid = rclone 未说明其配置所在的位置

# draft
remote_rclone_connected_already = 它已连接

# draft
remote_rclone_connection_refused = 服务器拒绝了连接

# draft
remote_rclone_continue = 继续

# draft
remote_rclone_edit = 编辑{ $remote }

# draft
remote_rclone_include = 包含{ $remote }

# draft
remote_rclone_keep_it = 保留

# draft
remote_rclone_keeps_no_link = “{ $path }”：rclone 不保存链接

# draft
remote_rclone_kind_detail = { $kind }：rclone 远程

# draft
remote_rclone_kind_field = 存储种类

# draft
remote_rclone_login_refused = 服务器拒绝了登录（密码错误？）

# draft
remote_rclone_login_refused_rc = 其登录被拒绝

# draft
remote_rclone_mount_late = rclone 未能及时挂载它

# draft
remote_rclone_name = 名称

# draft
remote_rclone_name_end = 远程的名称不能以空格结尾

# draft
remote_rclone_name_help = 侧边栏和 rclone 对它的称呼：字母 a–z、数字以及 _ - . +

# draft
remote_rclone_name_letters = 远程的名称只能包含字母、数字、空格以及 _ - . + @

# draft
remote_rclone_name_needed = 远程需要一个名称

# draft
remote_rclone_name_start = 远程的名称不能以 - 或空格开头

# draft
remote_rclone_name_taken = rclone 中已有名为{ $remote }的远程

# draft
remote_rclone_next = 下一步

# draft
remote_rclone_no_answer = { $remote }没有响应

# draft
remote_rclone_no_kind = rclone 中没有存储种类“{ $kind }”

# draft
remote_rclone_no_link = rclone 未生成链接

# draft
remote_rclone_no_log_folder = 没有用于存放其日志的文件夹

# draft
remote_rclone_no_mounts_folder = 没有可用于挂载的文件夹

# draft
remote_rclone_no_option = { $kind } 没有选项“{ $option }”

# draft
remote_rclone_no_reason = 未给出原因

# draft
remote_rclone_no_remote = 没有挂载任何远程

# draft
remote_rclone_no_remote_named = rclone 的配置中没有远程{ $remote }

# draft
remote_rclone_no_runtime_folder = 没有运行时文件夹

# draft
remote_rclone_no_such_host = 未找到服务器的名称

# draft
remote_rclone_no_such_name = rclone 不接受这样的名称

# draft
remote_rclone_no_such_remote = rclone 的配置中没有这样的远程

# draft
remote_rclone_not_asked = 它不是由 Skwir 挂载的，因此无法询问 rclone

# draft
remote_rclone_not_obscured = rclone 未混淆该密码

# draft
remote_rclone_not_ours = 它不是由 Skwir 挂载的，因此 Skwir 无法卸载它

# draft
remote_rclone_not_reached = 无法联系 rclone 以卸载它

# draft
remote_rclone_not_serving = rclone 未开始提供其配置

# draft
remote_rclone_ok = 确定

# draft
remote_rclone_on_remotes = 基于其他远程

# draft
remote_rclone_option_advanced = “{ $option }”是高级选项：由用户在窗口中设置

# draft
remote_rclone_option_secret = “{ $option }”是机密：由用户在窗口中输入

# draft
remote_rclone_provider = 提供商

# draft
remote_rclone_rc_login = rclone 拒绝了 { $method }：其登录被拒绝

# draft
remote_rclone_rc_no_http = rclone 对 { $method } 的回应不是 HTTP

# draft
remote_rclone_rc_refused = rclone 拒绝了 { $method }：{ $why }

# draft
remote_rclone_remove = 移除

# draft
remote_rclone_remove_intro = rclone 的配置将忘记{ $remote }：其中的内容仍保留在原处，重新添加即可恢复。

# draft
remote_rclone_remove_mounted_intro = rclone 的配置将忘记{ $remote }：其中的内容仍保留在原处，重新添加即可恢复。Skwir 会先断开它。

# draft
remote_rclone_remove_title = 要移除{ $remote }吗？

# draft
remote_rclone_save = 保存

# draft
remote_rclone_scheme = rclone 无法打开 { $scheme } 地址

# draft
remote_rclone_server_label = { $name } ({ $kind })

# draft
remote_rclone_set_up = 设置{ $remote }

# draft
remote_rclone_sign_in = 登录{ $remote }

# draft
remote_rclone_sign_in_page = 打开登录页面

# draft
remote_rclone_sign_in_waiting = 请在浏览器中完成登录：rclone 正在等待回应。如果没有打开页面，请在此处打开。

# draft
remote_rclone_stopped = rclone 已停止

# draft
remote_rclone_storage = 存储

# draft
remote_rclone_trash_box = Box 的回收站

# draft
remote_rclone_trash_drive = Google Drive 的回收站

# draft
remote_rclone_trash_jottacloud = Jottacloud 的回收站

# draft
remote_rclone_trash_mega = MEGA 的回收站

# draft
remote_rclone_trash_onedrive = OneDrive 的回收站

# draft
remote_rclone_trash_pcloud = pCloud 的回收站

# draft
remote_rclone_trash_yandex = Yandex Disk 的回收站

# draft
remote_rclone_trying = 正在尝试{ $remote }

# draft
remote_rclone_trying_what = rclone 会像连接时那样列出其顶层。

# draft
remote_rclone_unlisted = rclone 无法列出它：{ $why }。请保留原样，或返回并修改。

# draft
remote_rclone_unlisted_new = rclone 无法列出它：{ $why }。请保留原样，或返回并修改。“取消”按钮会将其移除。

# draft
remote_rclone_unmount_late = rclone 未能卸载它

# draft
remote_rclone_unsaid = “{ $path }”无法传给 rclone

# draft
remote_rclone_you_cancelled = 您已取消

# draft
remote_refresh = 刷新远程文件夹

# draft
remote_refresh_description = 重新读取已挂载的内容以及各工具所知的服务器

# draft
remote_remembered_data = 密码保存在密钥环中的服务器，按其地址

# draft
remote_rsync_between = rsync 在本机与服务器之间复制

# draft
remote_rsync_failed = rsync 失败（{ $status }）：{ $said }

# draft
remote_rsync_unnamed = “{ $path }”的名称无法传给 rsync

# draft
remote_runs_failed = { $tool } 失败（{ $status }）：{ $said }

# draft
remote_sending = 正在将 { $count } 个文件发送到服务器…

# draft
remote_server_form_add = 添加

# draft
remote_server_form_add_afp = 添加 AFP 卷

# draft
remote_server_form_add_dav = 添加 WebDAV 服务器

# draft
remote_server_form_add_davs = 添加 WebDAV 服务器 (HTTPS)

# draft
remote_server_form_add_ftp = 添加 FTP 服务器

# draft
remote_server_form_add_ftps = 添加 FTP 服务器 (TLS)

# draft
remote_server_form_add_intro = 通过地址添加服务器。安装 rclone 后，还可添加七十多种存储。

# draft
remote_server_form_add_nfs = 添加 NFS 导出

# draft
remote_server_form_add_other = 添加 { $kind }

# draft
remote_server_form_add_server = 添加服务器

# draft
remote_server_form_add_sftp = 添加 SFTP 服务器

# draft
remote_server_form_add_smb = 添加 Windows 共享

# draft
remote_server_form_afp = AFP 卷

# draft
remote_server_form_afp_detail = 较旧的 Mac 或 NAS 的共享卷

# draft
remote_server_form_already = { $server }已是该地址上的服务器

# draft
remote_server_form_dav = WebDAV 服务器

# draft
remote_server_form_dav_detail = Nextcloud、ownCloud 等，通过 HTTP

# draft
remote_server_form_davs = WebDAV 服务器 (HTTPS)

# draft
remote_server_form_davs_detail = Nextcloud、ownCloud 等

# draft
remote_server_form_domain = 域

# draft
remote_server_form_domain_help = Windows 域或工作组，在服务器需要时填写

# draft
remote_server_form_edit = 编辑{ $server }

# draft
remote_server_form_edit_intro = 保存{ $kept }。其挂载选项会随它转到新地址。

# draft
remote_server_form_export = 导出

# draft
remote_server_form_export_help = 导出的文件夹在服务器上的路径：/srv/photos

# draft
remote_server_form_folder = 文件夹

# draft
remote_server_form_folder_help = 在服务器上打开的位置：留空表示其顶层（SFTP：您的主文件夹）

# draft
remote_server_form_ftp = FTP 服务器

# draft
remote_server_form_ftp_detail = 通过 FTP 访问服务器的文件

# draft
remote_server_form_ftps = FTP 服务器 (TLS)

# draft
remote_server_form_ftps_detail = 通过带 TLS 的 FTP 访问服务器的文件

# draft
remote_server_form_host = 服务器

# draft
remote_server_form_host_help = 其名称或地址：nas.local、files.example.com、192.168.1.20

# draft
remote_server_form_host_needed = 需要填写服务器

# draft
remote_server_form_intro_kde = 保存在 KDE 的网络文件夹中，Dolphin 也会列出它。首次连接时会询问其密码。

# draft
remote_server_form_intro_skwir = 保存在 Skwir 的服务器列表中。首次连接时会询问其密码。

# draft
remote_server_form_kind = 服务器种类

# draft
remote_server_form_name = 名称

# draft
remote_server_form_name_help = 侧边栏对它的称呼；留空时使用其地址本身的名称

# draft
remote_server_form_next = 下一步

# draft
remote_server_form_nfs = NFS 导出

# draft
remote_server_form_nfs_detail = Unix 服务器的共享文件夹

# draft
remote_server_form_no_part = 服务器的地址没有“{ $key }”部分

# draft
remote_server_form_port = 端口

# draft
remote_server_form_port_refused = 端口是 1 到 65535 之间的数字

# draft
remote_server_form_save = 保存

# draft
remote_server_form_section = 服务器（按地址）

# draft
remote_server_form_sftp = SFTP 服务器

# draft
remote_server_form_sftp_detail = 通过 ssh 访问的计算机

# draft
remote_server_form_share = 共享

# draft
remote_server_form_share_help = 共享文件夹的名称，可在 / 之后跟其中的文件夹

# draft
remote_server_form_smb = Windows 共享 (SMB)

# draft
remote_server_form_smb_detail = NAS 或计算机的共享文件夹

# draft
remote_server_form_user = 用户

# draft
remote_server_form_user_help = 您登录时使用的名称；留空则使用您自己的用户名（SFTP：按您的 ssh 配置）

# draft
remote_server_form_volume = 卷

# draft
remote_server_form_volume_help = 共享卷的名称，可在 / 之后跟其中的文件夹

# draft
remote_servers_kde_read_only = KDE 的网络文件夹是只读的；请在管理它的地方修改

# draft
remote_servers_kept_automatic = 自动

# draft
remote_servers_kept_dolphin = 在 Dolphin 的位置中

# draft
remote_servers_kept_fstab = 在系统的 fstab 中

# draft
remote_servers_kept_in = 服务器的保存位置

# draft
remote_servers_kept_in_description = 通过地址添加的服务器保存在何处。

# draft
remote_servers_kept_in_description_more = 自动：在 KDE 桌面上保存在 KDE 的网络文件夹中（Dolphin 也会列出它们），在其他桌面上保存在 Skwir 自己的列表中；KIO 无法打开的种类（FTPS、AFP）始终保存在 Skwir 的列表中。

# draft
remote_servers_kept_kde = 在 KDE 的网络文件夹中

# draft
remote_servers_kept_rclone = 在 rclone 的配置中

# draft
remote_servers_kept_skwir = 在 Skwir 的列表中

# draft
remote_servers_kio_opens_no = KIO 无法打开 { $protocol } 地址

# draft
remote_servers_list = Skwir 自己保存的、通过地址添加的服务器：各自的名称和地址

# draft
remote_servers_no_kde_folder = 没有用于 KDE 网络文件夹的数据文件夹

# draft
remote_servers_none_kept = { $address } 处没有保存服务器

# draft
remote_servers_other_there = 该地址上已保存了另一个服务器

# draft
remote_servers_taken = { $name }已是该地址上的服务器

# draft
remote_sftp_from_here = sftp 从本机复制到服务器，或在服务器内部复制

# draft
remote_sftp_onto_server = sftp 复制到服务器

# draft
remote_sftp_unsaid = “{ $path }”无法传给 sftp

# draft
remote_share_label = { $server } 上的 { $share }

# draft
remote_ssh_still_there = “{ $path }”仍然存在

# draft
remote_sshfs_connection_refused = 服务器拒绝了连接

# draft
remote_sshfs_identity_changed = 服务器的身份自上次连接以来已更改：ssh 拒绝连接（请参阅 ~/.ssh/known_hosts）

# draft
remote_sshfs_identity_unconfirmed = 服务器的身份未经确认

# draft
remote_sshfs_login_refused = 服务器拒绝了登录（密码错误，或没有它接受的密钥）

# draft
remote_sshfs_name_not_found = 未找到服务器的名称

# draft
remote_sshfs_no_answer = 服务器没有响应

# draft
remote_sshfs_not_windows = sshfs 无法在 Windows 上运行

# draft
remote_system_shares_elsewhere = 系统自带的共享属于 Windows 和 macOS：此处由 sshfs、GVfs、KIO 或 rclone 挂载它们

# draft
remote_system_shares_login_refused = 服务器拒绝了登录

# draft
remote_system_shares_no_share = “{ $address }”未指定共享：smb://server/share

# draft
remote_thumbnails = 远程文件夹中的缩略图

# draft
remote_thumbnails_always = 总是

# draft
remote_thumbnails_automatic = 自动

# draft
remote_thumbnails_choice = 缩略图

# draft
remote_thumbnails_description = 适用于未在其菜单中作出选择的远程文件夹：“自动”仅在本地网络上显示缩略图，因为通过互联网读取每张图片会很慢。

# draft
remote_thumbnails_menu_always = 总是

# draft
remote_thumbnails_menu_automatic = 自动

# draft
remote_thumbnails_menu_never = 从不

# draft
remote_thumbnails_menu_small = 小文件

# draft
remote_thumbnails_never = 从不

# draft
remote_thumbnails_not_saved = 未能保存远程文件夹何时显示缩略图的选择：{ $error }

# draft
remote_thumbnails_of = 远程文件夹的缩略图

# draft
remote_thumbnails_of_description = 选择远程文件夹中的文件何时显示缩略图：automatic、never、small 或 always

# draft
remote_thumbnails_rule = 规则可以是 automatic、never、small 或 always

# draft
remote_thumbnails_small = 小文件

# draft
remote_thumbnails_submenu = 缩略图

# draft
remote_tool_failed = { $tool }：{ $error }

# draft
remote_tool_unknown = Skwir 不知道挂载它的工具

# draft
remote_tools_failed = { $failures }

# draft
remote_under_way = 那里仍有操作正在进行：{ $actions }

# draft
remote_under_way_title = “{ $title }”

# draft
remote_unfinished_data = Skwir 开始设置但未完成的 rclone 远程，按名称

# draft
remote_uploads_mount_gone = rclone 对“{ $path }”的挂载在其服务器收到写入的内容之前就已消失

# draft
remote_uploads_never_queued = “{ $path }”从未进入 rclone 的队列：某个程序仍在那里打开着它，服务器上没有它

# draft
remote_uploads_no_answer = rclone 在 { $seconds } 秒内没有响应：其服务器可能已停止响应

# draft
remote_uploads_no_cache_mode = rclone 未说明它使用哪种缓存

# draft
remote_uploads_refused = 服务器 { $tries } 次拒绝了“{ $name }”：rclone 将它保留在此磁盘上并会重试

# draft
remote_winfsp_loopback = Windows 上的挂载在环回地址上响应

# draft
remote_winfsp_missing = 未安装 WinFsp：rclone 在 Windows 上通过它挂载

# draft
remote_winfsp_no_login_folder = 没有用于存放其登录信息的文件夹
