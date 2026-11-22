# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# A setting's title: how much memory thumbnails may take, in megabytes.
thumbnails_budget = Memory budget

thumbnails_budget_description = Decoded thumbnails kept in memory.

# What a setting's description says after its first sentence, in its tooltip.
thumbnails_budget_description_more = Higher values scroll large image folders more smoothly.

# A setting's title. ImageMagick is a program's name.
thumbnails_imagemagick = Use ImageMagick

# ImageMagick is a program's name.
thumbnails_imagemagick_description = Draw with ImageMagick, when it is installed, the thumbnails and previews of the pictures no thumbnailer draws.

# Photoshop and GIMP are programs' names; HEIC and AVIF are formats' names. Skwir's HEIF helper is the program Skwir reads HEIC and AVIF pictures with, installed apart (skwir-heif).
thumbnails_imagemagick_description_more = It reads Photoshop and GIMP images, the camera raw files Skwir does not read, HEIC and AVIF where Skwir's HEIF helper is not installed, and more, isolated from your files and the network.

# An error in place of a picture's preview: no installed program (a thumbnailer, ImageMagick) draws its type.
thumbnails_none_draws = No thumbnailer draws this picture

thumbnails_plugin_description = Thumbnails from the desktop cache, an image decoder, installed thumbnailers and ImageMagick

thumbnails_plugin_title = Thumbnails

# What stops working without ImageMagick, in the settings' list of what a plugin needs. Photoshop and GIMP are programs' names.
thumbnails_requirement_magick = Pictures no thumbnailer draws (Photoshop and GIMP images, some raw files…)

# What stops working without the sandbox program (bubblewrap), in the settings' list of what a plugin needs: thumbnailers and ImageMagick are not run without it.
thumbnails_requirement_sandbox = Thumbnailers and ImageMagick run isolated

# A setting's title: whether thumbnails Skwir makes are kept where other applications find them.
thumbnails_write_cache = Share generated thumbnails

thumbnails_write_cache_description = Store new thumbnails in the desktop's thumbnail cache so other applications reuse them.
