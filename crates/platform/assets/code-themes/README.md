# Code themes Skwir carries

The code themes of Skwir's palettes that bat does not ship (ADR-071,
amended 2026-11-05), compiled into `skwir-platform` (`highlight::CARRIED`)
and offered on the Reading page beside bat's. Each is its makers' own,
under the licence beside it, which the packages' notices carry
(`tools/package/notices.py`). Fetched 2026-11-05.

| File | From | Licence |
| --- | --- | --- |
| `rose-pine.tmTheme`, `rose-pine-dawn.tmTheme` | [rose-pine/tm-theme](https://github.com/rose-pine/tm-theme), `dist/` | MIT, Rosé Pine (`rose-pine.LICENSE`) |
| `tokyonight_night.tmTheme`, `tokyonight_day.tmTheme` | [folke/tokyonight.nvim](https://github.com/folke/tokyonight.nvim), `extras/sublime/` | Apache-2.0, Folke Lemaitre (`tokyonight.LICENSE`) |
| `everforest-dark.tmTheme`, `everforest-light.tmTheme` | [mhanberg/everforest-textmate](https://github.com/mhanberg/everforest-textmate) (archived; the only medium-contrast TextMate port, every color Everforest's own) | MIT, Mitchell Hanberg (`everforest-textmate.LICENSE`) |

Changed here, as each file's first comment says: both Tokyo Night files
were given an XML declaration, and the Day file's colors that its
generator left from the Night style (text in HTML, punctuation of
blocks, tables, a warning token, a second-level heading, a cyan, a red,
an orange, a magenta, the caret and the selections), unreadable on its
ground at 1.04 to 1.9:1, were replaced by their Day counterparts. The
others are as fetched.
