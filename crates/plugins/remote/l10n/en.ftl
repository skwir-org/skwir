# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# text: what an address of a phone or a camera names as its device.
remote_address_bad_device = “{ $text }” is no device's name

# port: what was typed after the server's name and a colon.
remote_address_bad_port = “{ $port }” is no port

# name: what was typed as the name of a remote of rclone's.
remote_address_bad_remote = “{ $name }” is no remote's name

# host: what was typed as a server's name or address.
remote_address_bad_server = “{ $host }” is no server's name

# user: the user's name an address names.
remote_address_bad_ssh_user = “{ $user }” is no user's name ssh can be given

# user: what was typed as the user's name (or an SMB domain).
remote_address_bad_user = “{ $user }” is no user's name

# text: what an address of a phone or a camera names as its device.
remote_address_device_user = “{ $text }”: a device's address names no user

# An error: an address's server opens an IPv6 address with [ but never closes it with ]. text: what was typed as the server's address and port.
remote_address_ipv6 = “{ $text }” has no ] to end its IPv6 address

# text: what was typed as an address.
remote_address_no_protocol = “{ $text }” names no protocol

# text: what was typed as an address.
remote_address_no_server = “{ $text }” names no server

# text: what was typed as an address.
remote_address_no_slashes = “{ $text }” needs // before the server

# scheme: what an address names before its colon (sftp, smb).
remote_address_not_protocol = “{ $scheme }:” is not a protocol Skwir opens

remote_address_password = an address holds no password: the server asks for it

# The button of a question asked while connecting to a server (a password).
remote_ask_connect = Connect

# What a password prompt asks when the program asking (rclone) said nothing of its own.
remote_ask_password = Password

# What an S3 bucket is called: bucket is its name, profile the AWS profile it is reached by (both as written).
remote_bucket_label = { $bucket } ({ $profile })

# Why a connection or a transfer did not go on: it was stopped.
remote_cancelled = cancelled

remote_chosen_data = The thumbnails chosen for each remote, by its address

remote_configure_add_as_server = Add as Server…

# The button adding a server to Settings' list of the sidebar's Network servers.
remote_configure_add_record = Add Server

remote_configure_add_server = Add server…

remote_configure_add_server_description = Add a server: a remote of rclone's (cloud storage, S3, WebDAV, SFTP and seventy more), in a form the user fills; what it starts from may be given, never a secret

remote_configure_add_server_menu = Add Server…

# server: the name of the server just added.
remote_configure_added = { $server } is added

# server: the name of the remote just added to rclone's configuration.
remote_configure_added_obscured = { $server } is added. rclone keeps its secrets only obscured: encrypt its configuration?

# server: the name of a remote of rclone's, which was tested.
remote_configure_answers = { $server } answers

remote_configure_at_login = Connect at Login

# remotes: the names of the remotes of rclone's that connect at login, each quoted, as a list.
remote_configure_at_login_encrypted = { $count ->
        [1] { $remotes } connects at login, which an encrypted configuration cannot: nobody is there to give its password. Turn Connect at Login off first.
       *[other] { $remotes } connect at login, which an encrypted configuration cannot: nobody is there to give its password. Turn Connect at Login off first.
    }

# server: the login and server, or a remote of rclone's.
remote_configure_at_login_failed = { $server } was not set to connect at login: { $error }

# A button on a server's line in Settings, while it connects at login: the mark says it does.
remote_configure_at_login_on = Connect at Login ✓

remote_configure_at_login_which = only rclone's remotes and SFTP servers connect at login

remote_configure_bookmark_stays = its bookmark stays: { $error }

remote_configure_connect_at_login = Connect at login

remote_configure_connect_at_login_description = Have the system's service manager connect a server when the user logs in (a systemd user unit, a launch agent, a logon task), or no longer; connects it now too. Only what asks nothing at login: a remote of rclone's configuration not encrypted, an SFTP server logged in to by key

# server: the login and server, or a remote of rclone's.
remote_configure_connects_at_login = { $server } connects at login

# What was put on the clipboard, placed in the window's “Copied …”.
remote_configure_copied_what = the public link

remote_configure_copy_public_link = Copy Public Link

# server: the name of a remote of rclone's, which was tested.
remote_configure_does_not_answer = { $server } does not answer: { $error }

remote_configure_edit_server = Edit server…

remote_configure_edit_server_description = Change a server in its form: a remote of rclone's, or one added by its address

remote_configure_edit_server_menu = Edit Server…

remote_configure_encrypt = Encrypt rclone's configuration…

remote_configure_encrypt_accept = Encrypt

# The field where the password is typed a second time.
remote_configure_encrypt_again = The Same Again

# The button that encrypts rclone's configuration.
remote_configure_encrypt_button = Encrypt…

remote_configure_encrypt_description = Encrypt rclone's configuration file, which keeps its remotes' secrets only obscured, with a password the user types and may keep in the keyring

remote_configure_encrypt_intro = rclone keeps its remotes' keys and passwords in its configuration file, only obscured: anyone who reads the file can use them. Encrypted, it opens with a password.

remote_configure_encrypt_keep = Keep It in the Keyring

remote_configure_encrypt_keep_help = Skwir then opens the configuration without asking. rclone in a terminal asks for it, or reads it from RCLONE_CONFIG_PASS.

remote_configure_encrypt_password = Password

remote_configure_encrypt_title = Encrypt rclone's Configuration

remote_configure_encrypted = rclone's configuration is encrypted

remote_configure_encrypted_already = rclone's configuration is encrypted already

# A menu item of a remote of rclone's whose setting up was not finished.
remote_configure_finish = Finish Setting Up…

remote_configure_forget_password = Forget Password

remote_configure_keep_as_server = Keep as server

remote_configure_keep_as_server_description = Make a remote folder's address a server of the Network section, kept in Skwir's list or KDE's network folders, under its bookmark's name; its bookmark, if it has one, is taken out

remote_configure_keep_as_server_item = Keep as Server

remote_configure_keep_as_server_menu = Offer Keep as Server in a place's menu

remote_configure_keep_as_server_menu_description = A sidebar place's Keep as Server, for a remote folder's address that is no server yet (a bookmark's). Answers from memory

# kept: where it is kept (in Skwir's list, in KDE's network folders).
remote_configure_kept_as_server = { $server } is a server of the Network section, kept { $kept }

# protocol: rclone or S3.
remote_configure_kept_by_tools = { $protocol } is kept by rclone or the AWS tools

# kept: where the server is kept (in rclone's configuration, one of Dolphin's places).
remote_configure_kept_elsewhere = { $server } is { $kept }: change it there

# server: the login and server (me@nas); what was kept: its mount options, its entry connecting it at login.
remote_configure_kept_stays = What Skwir kept for { $server } stays: { $error }

remote_configure_key_login = Set Up Key Login…

# server: the login and server (me@nas).
remote_configure_key_login_failed = Key login was not set up on { $server }: { $error }

remote_configure_key_login_sftp = key login is ssh's: SFTP servers alone

remote_configure_mount_options = Mount options…

remote_configure_mount_options_button = Mount Options…

remote_configure_mount_options_description = Choose how a server is mounted when it next connects: read only, rclone's file cache and bandwidth, sshfs's compression

# server: the name of the server just changed.
remote_configure_moved_off_login = { $server } no longer connects at login: its address changed

remote_configure_nearby_menu = Nearby server's menu

remote_configure_nearby_menu_description = The context menu's items for the one server selected in the Nearby place: Add as Server…, its form filled in with the name and address it announced, where no server holds that address. Answers from memory

# server: the login and server, or a remote of rclone's.
remote_configure_no_longer_at_login = { $server } no longer connects at login

# protocol: a protocol's name (NFS, AFP).
remote_configure_no_options = no tool Skwir mounts { $protocol } with takes options

remote_configure_no_public_link = No public link was made: { $error }

# What was to be done to a server (done: added, saved, signed in, tested, kept, removed) failed for error.
remote_configure_not_done = { $done ->
        [added] The server was not added: { $error }
        [saved] The server was not saved: { $error }
        [signed_in] Signing in to the server failed: { $error }
        [tested] The server was not tested: { $error }
        [kept] The server was not kept: { $error }
       *[removed] The server was not removed: { $error }
    }

remote_configure_not_encrypted = rclone's configuration was not encrypted: { $error }

remote_configure_not_remote = it is not in a remote folder

remote_configure_nothing_to_link = nothing to link to

remote_configure_options_not_at_login = The mount options were saved, but connecting at login mounts as before: { $error }

# server: the login and server (me@nas).
remote_configure_options_saved = { $server } is mounted so when it next connects

remote_configure_options_unsaved = The mount options were not saved: { $error }

# server: the login and server (me@nas).
remote_configure_password_stays = The password Skwir keeps for { $server } stays in the keyring: { $error }

remote_configure_passwords_differ = The two passwords differ

remote_configure_public_link = Copy public link

remote_configure_public_link_description = Make a public link to a file of a remote whose storage makes one (a cloud's share link), and put it on the clipboard

remote_configure_public_link_menu = Offer a public link in the context menu

remote_configure_public_link_menu_description = The context menu's Copy Public Link, for a file of a remote whose storage makes one. Answers from memory

remote_configure_reconnect = Sign in again…

remote_configure_reconnect_description = Sign a remote of rclone's in again through the browser, its access having expired or been revoked

# A server's line in Settings: kind is its type of storage (drive, s3) or its address; kept where it is kept (in Skwir's list).
remote_configure_record_kept = { $kind } · { $kept }

# What kind of storage a server is, and where it is kept.
remote_configure_record_kind = Kind

remote_configure_record_name = Name

# A server's line in Settings: kind is rclone's type of storage (drive, s3); kept where it is kept (in rclone's configuration).
remote_configure_record_unfinished = { $kind } · { $kept } · not finished setting up

remote_configure_remove = Remove

# kept: where the server is kept (in Skwir's list, in KDE's network folders).
remote_configure_remove_intro = Skwir forgets the server, kept { $kept }: what it holds stays where it is, and adding it again brings it back.

# kept: where the server is kept (in Skwir's list, in KDE's network folders).
remote_configure_remove_intro_password = Skwir forgets the server, kept { $kept }, and the password it keeps for it in the keyring: what it holds stays where it is, and adding it again brings it back.

remote_configure_remove_server = Remove server…

remote_configure_remove_server_description = Remove a server once the user confirms, where it is kept (rclone's configuration, Skwir's list, KDE's network folders), with its mount options and its entry connecting it at login: what it holds stays where it is

remote_configure_remove_server_menu = Remove Server…

remote_configure_remove_title = Remove { $server }?

# server: the name of the server just removed.
remote_configure_removed = { $server } is removed

# server: the name of the server just changed.
remote_configure_saved = { $server } is saved

remote_configure_server_records = Servers

remote_configure_server_records_description = Every server as Settings lists it: each one's name, kind and where it is kept, what its line does, and whether Skwir changes it (`fixed`: another program does)

remote_configure_servers = Servers

remote_configure_servers_description = The servers of the sidebar's Network section, each changed where it is kept.

# What a setting's description says after its first sentence, in its tooltip.
remote_configure_servers_description_more = Those added by their address are in Skwir's list or KDE's network folders; rclone's remotes in rclone's own configuration, where rclone in a terminal finds them too.

remote_configure_setup_key_login = Set up key login…

remote_configure_setup_key_login_description = Let ssh log in to an SFTP server by key, no password: a key the ssh agent holds, the user's default one, or a new one, its public half added to the server's authorized keys once the user chose it

# Signs a remote of rclone's in again through the browser.
remote_configure_sign_in_again = Sign In Again…

# server: the name of a remote of rclone's.
remote_configure_signed_in = { $server } is signed in again

# Why a remote was not removed: letting go of its mount failed (error).
remote_configure_stays_mounted = it stays mounted: { $error }

# A button on a server's line in Settings: asks whether it answers.
remote_configure_test = Test

remote_configure_test_server = Test server

remote_configure_test_server_description = List the top of a remote of rclone's, as connecting would, and say whether it answers

remote_configure_test_server_menu = Test Server

remote_configure_turned_off = remote folders are turned off

remote_configure_unknown_server = no server is known at { $address }

remote_configure_window_needed = this is asked in a form, filled in Skwir's window: open it first

remote_connect = Connect to server

remote_connect_description = Open a remote folder by its address (sftp://, smb://, dav://, ftp://, nfs://, afp://, rclone://)

remote_connectors = Tools that open addresses

# sshfs, kmtp, gvfs, kio, rclone and fstab are what the setting's value names: never translated.
remote_connectors_description = The tools that mount an sftp://, smb:// or other address, tried in this order among those installed: sshfs, kmtp (a phone KDE holds), gvfs, kio, rclone, fstab.

# sshfs, kmtp, gvfs, kio, rclone and fstab are what the setting's value names: never translated.
remote_connectors_description_more = On KDE's desktop, kio comes before gvfs.

# Why a device plugged in opens nowhere. programs: the programs that hold it, by their names; device: phone or camera.
remote_device_held = { $count ->
        [1] { $programs } has
       *[other] { $programs } have
    } it open, and only one program at a time can use { $device ->
        [phone] a phone
       *[camera] a camera
    }. Quit { $count ->
        [1] { $programs }
       *[other] them
    }, then try again.

# Why a device plugged in opens nowhere. programs: the programs that hold it, by their names; device: phone or camera; holder: the program a tool would open it through (kiod6); why: why that tool cannot here.
remote_device_held_through = { $count ->
        [1] { $programs } has
       *[other] { $programs } have
    } it open, and only one program at a time can use { $device ->
        [phone] a phone
       *[camera] a camera
    }. Skwir would open it through { $holder }, but { $why }; else quit { $count ->
        [1] { $programs }
       *[other] them
    }, then try again.

remote_device_not_mounted = Not mounted. Click to mount and open it.

remote_disconnect = Disconnect

remote_disconnect_description = Let go of a remote folder: what shows it goes home

# name: a remote folder's, as its place names it.
remote_disconnected = Disconnected from { $name }

# name: a remote folder's, as its place names it; count: the files other programs wrote there that its server has not been sent yet.
remote_disconnected_kept = Disconnected from { $name }: { $count ->
        [one] { $count } file
       *[other] { $count } files
    } not sent yet stay in rclone's cache, sent when it next connects

# Said on a remote folder's place while it is let go of.
remote_disconnecting = Disconnecting…

remote_forget_password = Forget password

remote_forget_password_description = Remove from the keyring the password Skwir keeps for a server (never one another program kept)

remote_forget_password_menu = Forget Password

# What mounts the network folders the system's table (fstab) names: a tool's name, as sshfs and GVfs are, said before what it could not do.
remote_fstab = the system's mounts

remote_fstab_administrator = the system mounted it: only an administrator may let go of it

remote_fstab_linux_only = fstab is read on Linux only

remote_fstab_no_entry = no entry of /etc/fstab that you may mount names it

# What a camera GVfs reads is called: device is its name or where it is plugged in.
remote_gvfs_camera_label = { $device } (camera)

# A question asked where the server cannot be vouched for; host: its name or address. What GVfs said is shown under it.
remote_gvfs_connect_to = Connect to { $host }?

# it: a phone or a camera, which GVfs was asked to open.
remote_gvfs_device_silent = it did not answer within { $seconds ->
        [one] { $seconds } second
       *[other] { $seconds } seconds
    }

# What an iPhone or iPad GVfs reads is called: device is its name or serial.
remote_gvfs_ios_label = { $device } (iOS)

remote_gvfs_linux_only = GVfs runs on Linux only

# What a phone GVfs reads over MTP is called: device is its name or serial.
remote_gvfs_mtp_label = { $device } (MTP)

remote_gvfs_no_address = GVfs says no address for this mount

# address: the server's address (sftp://example.com/).
remote_gvfs_no_bridge = GVfs mounted { $address } but its FUSE bridge is not running: is gvfsd-fuse installed?

# address: the server's address (sftp://example.com/).
remote_gvfs_no_folder = GVfs mounted { $address } but its folder is not there

# What a mount of GVfs no address names is called: host is its server, kind GVfs's name for its protocol (as written).
remote_gvfs_other_label = { $host } ({ $kind })

# host: the server's name or address, which GVfs was asked to open.
remote_gvfs_server_silent = { $host } did not answer within { $seconds ->
        [one] { $seconds } second
       *[other] { $seconds } seconds
    }

# Why a connection was not made: the user cancelled a question it asked.
remote_gvfs_you_cancelled = you cancelled

# name: a remote of rclone's, as its configuration names it. Finish Setting Up… is the item of its menu.
remote_half_made = { $name } is not finished setting up: Finish Setting Up… in its menu

remote_keys_already_there = there is a key at { $path } already

remote_keys_comment_line = a key's comment is one line

remote_keys_form_accept = Set Up

# The field where a new key's passphrase is typed a second time.
remote_keys_form_again = The Same Again

remote_keys_form_intro = Your key's public half is added to the server's authorized keys: ssh then logs in with it, no password. Its private half never leaves this machine. The server's password is asked once.

remote_keys_form_key = Key

remote_keys_form_passphrase = New Key's Passphrase

remote_keys_form_passphrase_help = Encrypts a new key: asked when it is used, unless the agent holds it. Empty for none.

# server: the login and server a key is set up for (me@nas).
remote_keys_form_title = Set Up Key Login on { $server }

# server: the login and server (me@nas).
remote_keys_had_it = { $server } had the key already: ssh logs in with it

# A key the ssh agent holds: key is its comment and type (me@laptop (ed25519)), or its type alone.
remote_keys_in_agent = { $key }, in the ssh agent

# A key kept in a file: key is its comment and type (me@laptop (ed25519)), or its type alone; file: the file's path.
remote_keys_in_file = { $key }, in { $file }

# An ssh key as a form lists it: its comment (me@laptop), then its type (ed25519), both as ssh writes them.
remote_keys_key = { $comment } ({ $kind })

# said: what ssh-keygen printed, as it wrote it.
remote_keys_keygen_failed = ssh-keygen failed: { $said }

remote_keys_keygen_no_passphrase = ssh-keygen was not given the passphrase

remote_keys_keygen_no_public = ssh-keygen wrote no public key

remote_keys_new = A new key

# Where a new key is made: folder is a path (~/.ssh).
remote_keys_new_made_in = Made in { $folder }

remote_keys_no_home = there is no home folder for ssh's keys

remote_keys_no_runtime = there is no runtime folder

remote_keys_no_such_key = no such key

remote_keys_none_chosen = no key was chosen

# path: where ssh's default key is.
remote_keys_none_offered = ssh offers no key, and a new one would be made over the one at { $path }

# why: what the server's shell printed last, as it wrote it.
remote_keys_not_added = the server could not add the keys: { $why }

remote_keys_not_added_unsaid = the server could not add the keys: it did not say why

# server: the login and server (me@nas).
remote_keys_not_taken = The key is on { $server }, but ssh does not log in with it: the server may not take keys

remote_keys_passphrases_differ = The two passphrases differ

# said: what sftp printed, as it wrote it.
remote_keys_refused = the server refused the keys: { $said }

# server: the login and server (me@nas).
remote_keys_set_up = Key login is set up on { $server }

remote_keys_sftp_only = key login is set up for SFTP servers

# said: what sftp printed, as it wrote it.
remote_keys_unread = the server's authorized keys could not be read: { $said }

# reason: what kio-fuse said, as the system words its error.
remote_kio_could_not = KIO could not open it: { $reason }

remote_kio_could_not_open = KIO could not open it

# Why a server KIO opened cannot be disconnected from.
remote_kio_keeps = KIO keeps its folders until the session ends

remote_kio_linux_only = KIO's folders (kio-fuse) are on Linux only

remote_kio_no_answer = kio-fuse did not answer

remote_kio_no_folder = kio-fuse mounted it but its folder is not there

# protocol: SFTP, SMB, WebDAV, FTP…
remote_kio_opens_no = KIO opens no { $protocol } address

remote_kmtp_not_held = KDE's kmtpd does not hold this phone

# A program of KDE's that holds phones plugged in over USB.
remote_kmtpd = KDE's kmtpd

remote_kmtpd_linux_only = KDE's kmtpd is reached on Linux only

# Why a server is not mounted at login on a Mac: the launch agent that does it is a link to a file that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it.
remote_login_agent_read_only = the login agent is read-only; change it where it is managed

# part: a path or an argument of the command that connects at login.
remote_login_control_character = “{ $part }” holds a character a service manager does not read

# it: the service manager's program; status: how it ended, as the system says it (exit status: 1).
remote_login_ended = it ended ({ $status })

# error: why systemd did not start the unit; unit: its name. The command in brackets is typed as it is.
remote_login_journal = { $error } (journalctl --user -u { $unit } says why)

remote_login_needs_password = the server wants a password or a key's passphrase: at login, nobody is there to give it (a key ssh's agent holds, or one with no passphrase, logs in alone)

remote_login_no_config = there is no configuration folder

remote_login_no_folder = there is no folder to mount in

remote_login_no_home = there is no home folder

# The folder a session keeps what it runs in ($XDG_RUNTIME_DIR).
remote_login_no_runtime = there is no runtime folder to mount in

remote_login_no_schtasks = the Task Scheduler's schtasks is missing

remote_login_no_systemd = systemd is not here to connect at login (no systemctl)

remote_login_no_user = Windows does not say who the user is (USERNAME)

# it: the server; why: what ssh said.
remote_login_not_reached = it was not reached, to know it logs in alone: { $why }

# part: a path or an argument of the command that connects at login.
remote_login_not_text = “{ $part }” is not text a service manager reads

# system: Linux, macOS or Windows.
remote_login_not_yet = Connecting at login is not available on { $system } yet

remote_login_rclone_encrypted = rclone's configuration is encrypted: at login, nobody is there to give its password

# remote: the name of one of rclone's remotes.
remote_login_rclone_no_remote = rclone's configuration names no remote { $remote }

# system: Linux, macOS or Windows.
remote_login_rclone_not_yet = Mounting rclone's remotes is not available on { $system } yet

remote_login_rclone_remotes_only = only a remote of rclone's configuration (rclone://name/) connects at login through rclone: nobody is there to give a server's password

remote_login_sshfs_sftp_only = sshfs connects SFTP addresses alone

# Why a server is not mounted at login: the systemd unit that does it is a link to a file that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it.
remote_login_unit_read_only = the login unit is read-only; change it where it is managed

remote_login_unknown_host = ssh does not know the server's identity yet: connect to it once first

remote_lost = The connection is lost: open it to connect again

remote_macfuse_missing = macFUSE or FUSE-T is not installed

remote_mount_options_menu = Mount Options…

remote_mounter_connected_already = it is connected already

# Why a mount failed: its program ended; status: how, as the system says it (exit status: 1).
remote_mounter_ended = it ended ({ $status })

remote_mounter_no_runtime_folder = there is no runtime folder to mount in

remote_mounter_no_runtime_folder_at_all = there is no runtime folder

remote_mounter_nothing_mounted = it said it mounted, but nothing is mounted there

remote_mounter_trust = Connect to this server?

# What keeps a mount from being let go of: programs, by the names they run under, a list; count: how many.
remote_mounter_used_by = { $count ->
        [1] { $programs } is using it
       *[other] { $programs } are using it
    }

remote_mounter_used_by_another = another program is using it

# Why a connection was not made: the user cancelled a question it asked.
remote_mounter_you_cancelled = you cancelled

# finder: Avahi or wsdd, what finds servers nearby; error: why it found none.
remote_nearby_browser_failed = { $finder }: { $error }

remote_nearby_empty = No server found nearby

remote_nearby_empty_detail = Servers that announce themselves on this network (a NAS, a Mac, a Windows PC) appear here, found by Avahi and wsdd while this place is shown.

# Why each finder of servers nearby found none, one after the other.
remote_nearby_failures = { $first }; { $then }

# The folder a session keeps what it runs in ($XDG_RUNTIME_DIR).
remote_nearby_no_runtime = no runtime folder

# why: what each of them said.
remote_nearby_none_answers = Servers nearby are found by Avahi or wsdd, and neither answers here ({ $why })

# system: Linux, macOS or Windows.
remote_nearby_not_yet = Finding servers nearby is not available on { $system } yet

# What a server found nearby is, in its row: protocol is SFTP, SMB, WebDAV, FTP, NFS or AFP.
remote_nearby_server_kind = { $protocol } server

# The place that lists the servers found on this network.
# noun
remote_nearby_title = Nearby

# status: how the program ended, as the system says it (exit status: 2).
remote_nearby_wsdd_ended = wsdd ended as it started ({ $status }): it may be too old to discover

remote_nearby_wsdd_no_start = wsdd did not start

remote_needs_gio = Opening addresses through GVfs, the desktop's remotes

remote_needs_kmtp = Opening a phone KDE's kmtpd holds

remote_needs_rclone = Opening rclone's remotes and other addresses

remote_needs_sshfs = Opening SFTP addresses through your ssh

# server: the server as the keyring knows it (smb://me@nas).
remote_no_password_kept = Skwir keeps no password for { $server }

# protocol: SFTP, SMB, MTP…
remote_no_tool = no tool Skwir knows opens { $protocol } addresses

# name: a remote folder's, as its place names it.
remote_not_disconnected = { $name } was not disconnected: { $error }

# program: a program's name, as it is installed (sshfs, rclone, kio-fuse (KDE)).
remote_not_installed = { $program } is not installed

remote_not_mounted_there = no remote folder is mounted there

# why: why each tool that opens such addresses cannot here, a list (sshfs is not installed and rclone is not installed).
remote_nothing_installed = nothing installed can open it: { $why }

remote_options_bandwidth = Bandwidth Limit

remote_options_bandwidth_help = rclone's: 10M, or an upload's and a download's as 10M:1M; empty for none

remote_options_bandwidth_refused = A bandwidth is a rate, as 10M or 512k, or an upload's and a download's, as 10M:1M

remote_options_cache = File Cache

# A choice of rclone's file cache, beside rclone's own (full, writes, minimal, off): Skwir chooses.
remote_options_cache_automatic = automatic

# What Automatic chooses for this server, and why; cache: rclone's name for the cache chosen (full, minimal).
remote_options_cache_automatic_detail = { $cache } here: { $why ->
        [wraps] it encrypts or wraps other remotes, whose files Full would keep on this disk
        [near] it is on the local network
       *[far] what is read is read again from this disk
    }

# What rclone's file cache “full” does.
remote_options_cache_full = What is read and written is kept on this disk an hour: read again from it, sent after it is closed, tried again

remote_options_cache_help = What rclone keeps of its files on this disk

# What rclone's file cache “minimal” does.
remote_options_cache_minimal = Each read asks the server; a file written whole is on it once closed

# What rclone's file cache “off” does.
remote_options_cache_off = Nothing kept on this disk: some programs cannot write

# What rclone's file cache “writes” does.
remote_options_cache_writes = Files written are kept on this disk and sent after they are closed; each read asks the server

remote_options_compression = Compression

remote_options_compression_help = ssh compresses what it sends (sshfs): faster on a slow network, slower on a fast one

remote_options_data = The mount options chosen for each server, by its address

remote_options_intro = Used when it next connects, by rclone or sshfs. GVfs and KIO mount as the desktop does.

remote_options_read_only = Read Only

remote_options_read_only_help = Nothing written there, by any program

remote_options_save = Save

# The title of a server's mount options; server: its login and name (me@nas).
remote_options_title = Mount Options of { $server }

# label: the login and server asked about (me@example.com).
remote_password_for = Password for { $label }

remote_password_forgotten = The password is forgotten

remote_password_not_forgotten = The password was not forgotten: { $error }

# why: what the keyring said (No keyring answered).
remote_password_not_kept = The password was not remembered: { $why }

remote_plugin_description = Open SFTP, SMB, WebDAV, FTP, NFS and rclone addresses as folders, through sshfs, GVfs, rclone or the system's mounts; phones and cameras through GVfs

remote_plugin_title = Remote folders

# host: the server's name or address.
remote_provider_agent_connects = An agent asks to connect to { $host }

# address: the address asked (sftp://example.com/srv).
remote_provider_agent_connects_detail = It asked to open { $address }. Connecting tells the server who you are.

# label: a phone's or a camera's name.
remote_provider_agent_opens = An agent asks to open { $label }

# address: the device's address (mtp://Example_Phone/).
remote_provider_agent_opens_detail = It asked to open { $address }.

remote_provider_camera_on = A camera shows its files while it is switched on.

# label: the server's, as the sidebar shows it (me@example.com).
remote_provider_connecting = Connecting to { $label }…

remote_provider_not_allowed = you did not allow it

# label: the server's, as the sidebar shows it; error: why.
remote_provider_not_connected = Could not connect to { $label }: { $error }

# label: a phone's or a camera's name; error: why.
remote_provider_not_opened = Could not open { $label }: { $error }

# label: a phone's or a camera's name, as the sidebar shows it.
remote_provider_opening = Opening { $label }…

remote_provider_phone_unlocked = A phone shows its files once it is unlocked.

remote_provider_quitting = Skwir is quitting

remote_provider_window_asks = If the server asks for a password, or to be trusted the first time, the window asks you.

# The button that adds a remote to rclone's configuration.
remote_rclone_add = Add

# A form's title. kind: a kind of storage as rclone names it (Amazon S3, Google Drive).
remote_rclone_add_kind = Add { $kind }

remote_rclone_add_server = Add Server

remote_rclone_add_server_intro = A server by its address, kept in Skwir's list or KDE's network folders, or a remote of rclone's, kept in rclone's own configuration where rclone in a terminal finds it too.

# The field answering one of rclone's questions, where rclone asks none in words.
remote_rclone_answer = Answer

# The button that gives the password of rclone's configuration, opening it.
remote_rclone_config_open = Open

remote_rclone_config_password = Password of rclone's configuration

remote_rclone_config_unsaid = rclone did not say where its configuration is

remote_rclone_connected_already = it is connected already

remote_rclone_connection_refused = the server refused the connection

remote_rclone_continue = Continue

# A form's title: remote is one of rclone's remotes, by its name.
remote_rclone_edit = Edit { $remote }

# A switch of a union or a combine of remotes: remote is one of rclone's remotes, by its name.
remote_rclone_include = Include { $remote }

remote_rclone_keep_it = Keep It

# path: a link's path, which a copy on the server cannot make.
remote_rclone_keeps_no_link = “{ $path }”: rclone keeps no link

# kind: what rclone says a kind of storage is, and its type, as rclone writes them (Amazon S3 Compliant Storage Providers (s3)).
remote_rclone_kind_detail = { $kind }: a remote of rclone's

remote_rclone_kind_field = Kind of storage

remote_rclone_login_refused = the server refused the login (wrong password?)

# Why rclone's remote control refused a call: the login Skwir gave it.
remote_rclone_login_refused_rc = its login was refused

remote_rclone_mount_late = rclone did not mount it in time

# The field naming a remote of rclone's.
remote_rclone_name = Name

remote_rclone_name_end = A remote's name ends with no space

# The signs after “digits and” are typed as they are: underscore, hyphen, full stop, plus.
remote_rclone_name_help = What the sidebar and rclone call it: letters a–z, digits and _ - . +

# The signs after “spaces and” are typed as they are: underscore, hyphen, full stop, plus, at sign.
remote_rclone_name_letters = A remote's name holds letters, digits, spaces and _ - . + @ only

remote_rclone_name_needed = A remote needs a name

remote_rclone_name_start = A remote's name starts with neither - nor a space

remote_rclone_name_taken = rclone has a remote named { $remote } already

# The button of a form that goes on to the next one.
remote_rclone_next = Next

# A form's title: a remote of rclone's could not be listed.
remote_rclone_no_answer = { $remote } Does Not Answer

# kind: a type of rclone's storage, as rclone names it (s3, drive).
remote_rclone_no_kind = rclone has no kind of storage “{ $kind }”

remote_rclone_no_link = rclone made no link

# Why rclone could not mount a server on Windows: Skwir has no folder for what it writes as it runs.
remote_rclone_no_log_folder = no folder for its log

remote_rclone_no_mounts_folder = there is no folder to mount in

# Read by agents. kind: a kind of storage as rclone names it (Amazon S3); option: one of its options, as rclone names it (endpoint).
remote_rclone_no_option = { $kind } has no option “{ $option }”

# rclone refused a call and said nothing of why.
remote_rclone_no_reason = no reason given

# Why rclone could not be asked about a folder it mounted: it says it mounts none.
remote_rclone_no_remote = no remote mounted

remote_rclone_no_remote_named = rclone's configuration names no remote { $remote }

# Why rclone's mount could not be given a remote control: the session has no folder for what runs in it.
remote_rclone_no_runtime_folder = no runtime folder

remote_rclone_no_such_host = the server's name was not found

remote_rclone_no_such_name = rclone takes no such name

remote_rclone_no_such_remote = rclone's configuration names no such remote

remote_rclone_not_asked = Skwir did not mount it, so rclone cannot be asked

remote_rclone_not_obscured = rclone did not obscure the password

remote_rclone_not_ours = Skwir did not mount it, so it cannot let go of it

# Said before why: rclone's remote control did not answer.
remote_rclone_not_reached = rclone was not reached to let go of it

remote_rclone_not_serving = rclone did not start serving its configuration

remote_rclone_ok = OK

# A heading among the kinds of storage to add: those that stand on other remotes (crypt, union).
remote_rclone_on_remotes = On other remotes

# Read by agents. option: an option of rclone's, as rclone names it.
remote_rclone_option_advanced = “{ $option }” is an advanced option: the user sets it in the window

# Read by agents. option: an option of rclone's, as rclone names it.
remote_rclone_option_secret = “{ $option }” is a secret: the user types it in the window

# Who provides a kind of storage (S3's: AWS, MinIO, Cloudflare).
remote_rclone_provider = Provider

# method: a call of rclone's remote control (operations/copyfile).
remote_rclone_rc_login = rclone refused { $method }: its login was refused

# method: a call of rclone's remote control (operations/copyfile).
remote_rclone_rc_no_http = rclone's answer to { $method } is no HTTP

# method: a call of rclone's remote control (operations/copyfile); why: what rclone said, as it wrote it.
remote_rclone_rc_refused = rclone refused { $method }: { $why }

remote_rclone_remove = Remove

remote_rclone_remove_intro = rclone's configuration forgets { $remote }: what it holds stays where it is, and adding it again brings it back.

remote_rclone_remove_mounted_intro = rclone's configuration forgets { $remote }: what it holds stays where it is, and adding it again brings it back. Skwir disconnects it first.

# A form's title: remote is one of rclone's remotes, by its name.
remote_rclone_remove_title = Remove { $remote }?

remote_rclone_save = Save

# scheme: a protocol's name (NFS, AFP, MTP).
remote_rclone_scheme = rclone does not open { $scheme } addresses

# What a remote of rclone's configuration is called: name is its name, kind rclone's type of storage (drive, sftp), both as written.
remote_rclone_server_label = { $name } ({ $kind })

# A form's title, where rclone asks a question about a remote it sets up: remote is its name.
remote_rclone_set_up = Set Up { $remote }

# A form's title while a browser's sign-in to a remote of rclone's is waited for.
remote_rclone_sign_in = Sign In to { $remote }

remote_rclone_sign_in_page = Open the Sign-In Page

remote_rclone_sign_in_waiting = Finish signing in in your browser: rclone waits for the answer. If no page opened, open it here.

remote_rclone_stopped = rclone stopped

# A heading among the kinds of storage to add: those that keep files.
remote_rclone_storage = Storage

# As remote_rclone_trash_drive: said inside a sentence, with its own determiner (English's possessive is one).
remote_rclone_trash_box = Box's trash

# Where a cloud keeps what is deleted, said inside a sentence, with its own determiner (English's possessive is one): the deleting dialog and its toasts place it bare (moved to Google Drive's trash).
remote_rclone_trash_drive = Google Drive's trash

# As remote_rclone_trash_drive: said inside a sentence, with its own determiner (English's possessive is one).
remote_rclone_trash_jottacloud = Jottacloud's trash

# As remote_rclone_trash_drive: said inside a sentence, with its own determiner (English's possessive is one).
remote_rclone_trash_mega = MEGA's rubbish bin

# As remote_rclone_trash_drive: said inside a sentence, with its own determiner (English's possessive is one).
remote_rclone_trash_onedrive = OneDrive's recycle bin

# As remote_rclone_trash_drive: said inside a sentence, with its own determiner (English's possessive is one).
remote_rclone_trash_pcloud = pCloud's trash

# As remote_rclone_trash_drive: said inside a sentence, with its own determiner (English's possessive is one).
remote_rclone_trash_yandex = Yandex Disk's trash

# A form's title while a remote of rclone's just made is tried.
remote_rclone_trying = Trying { $remote }

remote_rclone_trying_what = rclone lists its top, as connecting would.

# why: what rclone said, in its words or Skwir's.
remote_rclone_unlisted = rclone could not list it: { $why }. Keep it as it is, or go back and change it.

# A form's text, about a remote of rclone's just added. why: what rclone said, in its words or Skwir's. Cancel: the form's button, as it is named there.
remote_rclone_unlisted_new = rclone could not list it: { $why }. Keep it as it is, or go back and change it. The Cancel button removes it.

remote_rclone_unmount_late = rclone did not let go of it

# path: a path whose name is not text (not UTF-8), which rclone takes none of.
remote_rclone_unsaid = “{ $path }” cannot be said to rclone

# Why a connection did not go on: its password was not given.
remote_rclone_you_cancelled = you cancelled

remote_refresh = Refresh remote folders

remote_refresh_description = Read again what is mounted and which servers the tools know

remote_remembered_data = The servers whose password is kept in the keyring, by their address

remote_rsync_between = rsync copies between this machine and a server

# status: how rsync ended, as the system says it (exit status: 23); said: what rsync printed, in English.
remote_rsync_failed = rsync failed ({ $status }): { $said }

# path: a file's or a folder's path.
remote_rsync_unnamed = “{ $path }” has a name rsync cannot be given

# tool: a program's name (sftp, rsync); status: how it ended, as the system says it (exit status: 1); said: what it printed, in English.
remote_runs_failed = { $tool } failed ({ $status }): { $said }

# Said on a remote folder's place while what other programs wrote there is sent.
remote_sending = Sending { $count ->
        [one] { $count } file
       *[other] { $count } files
    } to the server…

remote_server_form_add = Add

remote_server_form_add_afp = Add AFP Volume

remote_server_form_add_dav = Add WebDAV Server

remote_server_form_add_davs = Add WebDAV Server (HTTPS)

remote_server_form_add_ftp = Add FTP Server

remote_server_form_add_ftps = Add FTP Server (TLS)

remote_server_form_add_intro = A server by its address. With rclone installed, seventy kinds of storage more.

remote_server_form_add_nfs = Add NFS Export

# kind: a protocol's name (rclone, S3).
remote_server_form_add_other = Add { $kind }

remote_server_form_add_server = Add Server

remote_server_form_add_sftp = Add SFTP Server

remote_server_form_add_smb = Add Windows Share

remote_server_form_afp = AFP Volume

remote_server_form_afp_detail = An older Mac's or NAS's shared volume

# server: the name of the server kept at that address.
remote_server_form_already = { $server } is a server already, at that address

remote_server_form_dav = WebDAV Server

remote_server_form_dav_detail = Nextcloud, ownCloud and others, over HTTP

remote_server_form_davs = WebDAV Server (HTTPS)

remote_server_form_davs_detail = Nextcloud, ownCloud and others

remote_server_form_domain = Domain

remote_server_form_domain_help = The Windows domain or workgroup, where the server wants one

remote_server_form_edit = Edit { $server }

# kept: where the server is kept (in Skwir's list, in KDE's network folders, in rclone's configuration).
remote_server_form_edit_intro = Kept { $kept }. Its mount options follow it to a new address.

remote_server_form_export = Export

remote_server_form_export_help = The exported folder's path on the server: /srv/photos

remote_server_form_folder = Folder

remote_server_form_folder_help = Where to open, on the server: empty for its top (SFTP: your home)

remote_server_form_ftp = FTP Server

remote_server_form_ftp_detail = A server's files over FTP

remote_server_form_ftps = FTP Server (TLS)

remote_server_form_ftps_detail = A server's files over FTP with TLS

remote_server_form_host = Server

remote_server_form_host_help = Its name or address: nas.local, files.example.com, 192.168.1.20

remote_server_form_host_needed = A server is needed

remote_server_form_intro_kde = Kept in KDE's network folders, where Dolphin lists it too. Its password is asked when it first connects.

remote_server_form_intro_skwir = Kept in Skwir's list of servers. Its password is asked when it first connects.

remote_server_form_kind = Kind of server

# The name a server is given in the sidebar.
remote_server_form_name = Name

remote_server_form_name_help = What the sidebar calls it; its address's own name where empty

remote_server_form_next = Next

remote_server_form_nfs = NFS Export

remote_server_form_nfs_detail = A Unix server's shared folder

# key: the name of a part a caller gave (host, port), as it gave it.
remote_server_form_no_part = a server's address has no part “{ $key }”

remote_server_form_port = Port

remote_server_form_port_refused = A port is a number from 1 to 65535

remote_server_form_save = Save

# A heading in Add Server's list of kinds of storage.
remote_server_form_section = Servers, by their address

remote_server_form_sftp = SFTP Server

remote_server_form_sftp_detail = A computer reached over ssh

remote_server_form_share = Share

remote_server_form_share_help = The shared folder's name, and a folder of it after a /

remote_server_form_smb = Windows Share (SMB)

remote_server_form_smb_detail = A NAS's or a computer's shared folder

remote_server_form_user = User

remote_server_form_user_help = The name you log in with; empty for your own (SFTP: as your ssh configuration says)

remote_server_form_volume = Volume

remote_server_form_volume_help = The shared volume's name, and a folder of it after a /

# Why a server was not added to KDE's network folders: its file is a link to one that cannot be written, as a configuration managed elsewhere (Nix's Home Manager) makes it.
remote_servers_kde_read_only = KDE's network folder is read-only; change it where it is managed

# Where servers are kept: as the desktop is.
remote_servers_kept_automatic = Automatic

# Where a server of the Network section is kept, said after its kind or address: Dolphin is KDE's file manager.
remote_servers_kept_dolphin = one of Dolphin's places

# Where a server of the Network section is kept, said after its kind or address: fstab is the system's table of mounts.
remote_servers_kept_fstab = in the system's fstab

remote_servers_kept_in = Where servers are kept

remote_servers_kept_in_description = Where a server added by its address is kept.

# What a setting's description says after its first sentence, in its tooltip.
remote_servers_kept_in_description_more = Automatic: in KDE's network folders on KDE's desktop, where Dolphin lists them too, and in Skwir's own list elsewhere; a kind KIO does not open (FTPS, AFP) always in Skwir's.

# Where a server of the Network section is kept, said after its kind or address.
remote_servers_kept_kde = in KDE's network folders

# Where a server of the Network section is kept, said after its kind or address.
remote_servers_kept_rclone = in rclone's configuration

# Where a server of the Network section is kept, said after its kind or address.
remote_servers_kept_skwir = in Skwir's list

# protocol: SFTP, SMB, WebDAV…
remote_servers_kio_opens_no = KIO opens no { $protocol } address

remote_servers_list = The servers added by their address that Skwir keeps itself: each one's name and address

remote_servers_no_kde_folder = there is no data folder for KDE's network folders

# address: a server's address (sftp://me@example.com).
remote_servers_none_kept = no server is kept at { $address }

remote_servers_other_there = another server is kept at that address

# name: the server's, as the Network section shows it.
remote_servers_taken = { $name } is a server already, at that address

remote_sftp_from_here = sftp copies from this machine onto a server, or within one

remote_sftp_onto_server = sftp copies onto a server

# path: a file's path, which sftp's commands cannot hold (a line break in it, or not UTF-8).
remote_sftp_unsaid = “{ $path }” cannot be said to sftp

# What a share of a server (SMB, AFP, an NFS export) is called in the sidebar and elsewhere: share is its name, server the server's (with its port where it is not the usual one), both as written.
remote_share_label = { $share } on { $server }

# Why removing failed: path is a file's path, still on the server.
remote_ssh_still_there = “{ $path }” is still there

remote_sshfs_connection_refused = the server refused the connection

remote_sshfs_identity_changed = the server's identity changed since it was last seen: ssh refuses it (see ~/.ssh/known_hosts)

remote_sshfs_identity_unconfirmed = the server's identity was not confirmed

remote_sshfs_login_refused = the server refused the login (wrong password, or no key it accepts)

remote_sshfs_name_not_found = the server's name was not found

remote_sshfs_no_answer = the server did not answer

remote_sshfs_not_windows = sshfs does not run on Windows

remote_system_shares_elsewhere = the system's own shares are Windows' and macOS's: here sshfs, GVfs, KIO or rclone mount them

remote_system_shares_login_refused = the server refused the login

# address: what was asked (smb://nas); after the colon, how an address names a share.
remote_system_shares_no_share = “{ $address }” names no share: smb://server/share

remote_thumbnails = Thumbnails in remote folders

remote_thumbnails_always = Always

remote_thumbnails_automatic = Automatic

# A place's menu: when its files show thumbnails.
remote_thumbnails_choice = Thumbnails

remote_thumbnails_description = For a remote you chose nothing for in its menu: Automatic shows them on the local network only, as reading every picture across the internet would be slow.

remote_thumbnails_menu_always = Always

remote_thumbnails_menu_automatic = Automatic

remote_thumbnails_menu_never = Never

remote_thumbnails_menu_small = Small Files

remote_thumbnails_never = Never

# An error: the choice made in a remote's Thumbnails menu (automatic, never, small files, always) was not written to the settings.
remote_thumbnails_not_saved = The choice of when the remote's files show thumbnails was not saved: { $error }

remote_thumbnails_of = Thumbnails of a remote

# automatic, never, small and always are the rules the command takes: never translated.
remote_thumbnails_of_description = Choose when a remote folder's files show thumbnails: automatic, never, small or always

# automatic, never, small and always are the rules the command takes: never translated.
remote_thumbnails_rule = a rule is automatic, never, small or always

remote_thumbnails_small = Small files

# A place's menu: the row of the submenu choosing when its files show thumbnails.
remote_thumbnails_submenu = Thumbnails

# What one tool could not do, among the failures of the tools tried. tool: its name (sshfs, GVfs); error: why.
remote_tool_failed = { $tool }: { $error }

remote_tool_unknown = Skwir does not know the tool that mounted it

# Why a server was not reached, after Could not connect to …: what each tool tried could not do (remote_tool_failed), a list (sshfs: the server refused the login and rclone: the server's name was not found).
remote_tools_failed = { $failures }

# Why a remote folder is not let go of: actions: the titles of what Skwir does there.
remote_under_way = { $count ->
        [1] { $actions } is under way there
       *[other] { $actions } are under way there
    }

# An action's title (copy 3 items to Docs), quoted.
remote_under_way_title = “{ $title }”

remote_unfinished_data = The remotes of rclone's Skwir began to set up and did not finish, by name

# path: a folder's path, in a folder rclone mounted.
remote_uploads_mount_gone = rclone's mount of “{ $path }” is gone before its server had what was written

# path: a file's path, in a folder rclone mounted.
remote_uploads_never_queued = “{ $path }” never reached rclone's queue: a program still holds it open there, and the server does not have it

# seconds: how long it was waited for; s: seconds.
remote_uploads_no_answer = rclone did not answer within { $seconds } s: its server may have stopped answering

remote_uploads_no_cache_mode = rclone did not say which cache it keeps

# name: a file's path in the remote, as rclone's queue names it; tries: how many times rclone sent it.
remote_uploads_refused = the server refused “{ $name }” { $tries ->
        [one] { $tries } time
       *[other] { $tries } times
    }: rclone keeps it on this disk and tries again

# Said where a mount's remote control was given a socket: on Windows it answers on this machine's own network address.
remote_winfsp_loopback = a mount on Windows answers on the loopback

remote_winfsp_missing = WinFsp is not installed: rclone mounts through it on Windows

# Why a mount of rclone's could not be made: no folder of Skwir's keeps the login of its remote control.
remote_winfsp_no_login_folder = no folder for its login
