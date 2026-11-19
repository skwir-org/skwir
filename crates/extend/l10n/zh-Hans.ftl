# Simplified Chinese catalog of skwir-extend, drafted by a model from en.ftl (ADR-139): every message is a draft until reviewed.

# draft
extend_answer_cut = { $start }…

# draft
extend_answered = { $command }：{ $value }

# draft
extend_arguments_object = 参数应为对象

# draft
extend_command_failed = { $command }：{ $error }

# draft
extend_converted_gone = 转换后的文件已不存在

# draft
extend_copy_answer = 复制

# draft
extend_did_not_start = 插件未能启动

# draft
extend_ending = 插件正在退出

# draft
extend_failed_before = 插件刚才未能启动

# draft
extend_file_not_base64 = 插件返回的文件不是 base64 编码

# draft
extend_install_module_worker = 安装 { $program } 以运行 WebAssembly 模块形式的插件

# draft
extend_may_not_write = 此插件无权更改文件

# draft
extend_no_answer = 插件未及时响应

# draft
extend_no_page = 没有第 { $page } 页：共有 { $pages } 页

# draft
extend_no_such_file = 此次调用中没有文件 { $handle }

# draft
extend_not_reading = 插件未读取发给它的请求

# draft
extend_page_not_base64 = 插件返回的页面不是 base64 编码

# draft
extend_pages_links_closed = 不会打开{ $plugin }所绘页面中的链接：该插件不得使用网络

# draft
extend_plugin_off = 插件已停用

# draft
extend_preview_links_closed = 不会打开{ $plugin }所生成预览中的链接：该插件不得使用网络

# draft
extend_private_file = 私密文件：外部插件无法看到它（设置 › 隐私）

# draft
extend_private_folder = 私密文件夹：插件无法访问它

# draft
extend_private_preview = 此文件为私密文件：外部插件不会读取它（设置 › 隐私）

# draft
extend_protocol_version = 该插件使用第 { $version } 版协议；Skwir 支持第 { $oldest } 至 { $newest } 版

# draft
extend_requirement_module = 运行此插件的 WebAssembly 模块

# draft
extend_shows_nothing = { $plugin }未显示此文件的任何内容

# draft
extend_stopped = 插件已停止运行

# draft
extend_thumbnail_not_base64 = 缩略图不是 base64 编码

# draft
extend_turned_off = { $count ->
        [1] 已停用插件{ $plugin }：它停止了响应
       *[other] 已停用插件{ $plugin }：它连续 { $count } 次停止响应
    }

# draft
extend_turned_off_action = 插件

# draft
extend_turned_off_meanwhile = 插件已被停用
