// Made from the crates' l10n folders by `just words` (tools/words/extract.py):
// never edit by hand. Every catalog but English, compiled in (ADR-139, G),
// and how much of each language they say (ADR-139, 13).

#[rustfmt::skip]
pub static CATALOGS: &[skwir_core::words::Shipped] = &[
    skwir_core::words::Shipped {
        domain: "skwir-core",
        language: "fr",
        source: include_str!("../../core/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-core",
        language: "it",
        source: include_str!("../../core/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-core",
        language: "zh-Hans",
        source: include_str!("../../core/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-extend",
        language: "fr",
        source: include_str!("../../extend/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-extend",
        language: "it",
        source: include_str!("../../extend/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-extend",
        language: "zh-Hans",
        source: include_str!("../../extend/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-platform",
        language: "fr",
        source: include_str!("../../platform/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-platform",
        language: "it",
        source: include_str!("../../platform/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-platform",
        language: "zh-Hans",
        source: include_str!("../../platform/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-archives",
        language: "fr",
        source: include_str!("../../plugins/archives/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-archives",
        language: "it",
        source: include_str!("../../plugins/archives/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-archives",
        language: "zh-Hans",
        source: include_str!("../../plugins/archives/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-books",
        language: "fr",
        source: include_str!("../../plugins/books/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-books",
        language: "it",
        source: include_str!("../../plugins/books/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-books",
        language: "zh-Hans",
        source: include_str!("../../plugins/books/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-checksums",
        language: "fr",
        source: include_str!("../../plugins/checksums/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-checksums",
        language: "it",
        source: include_str!("../../plugins/checksums/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-checksums",
        language: "zh-Hans",
        source: include_str!("../../plugins/checksums/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-circuits",
        language: "fr",
        source: include_str!("../../plugins/circuits/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-circuits",
        language: "it",
        source: include_str!("../../plugins/circuits/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-circuits",
        language: "zh-Hans",
        source: include_str!("../../plugins/circuits/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-comics",
        language: "fr",
        source: include_str!("../../plugins/comics/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-comics",
        language: "it",
        source: include_str!("../../plugins/comics/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-comics",
        language: "zh-Hans",
        source: include_str!("../../plugins/comics/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-control",
        language: "fr",
        source: include_str!("../../plugins/control/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-control",
        language: "it",
        source: include_str!("../../plugins/control/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-control",
        language: "zh-Hans",
        source: include_str!("../../plugins/control/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-djvu",
        language: "fr",
        source: include_str!("../../plugins/djvu/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-djvu",
        language: "it",
        source: include_str!("../../plugins/djvu/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-djvu",
        language: "zh-Hans",
        source: include_str!("../../plugins/djvu/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-documents",
        language: "fr",
        source: include_str!("../../plugins/documents/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-documents",
        language: "it",
        source: include_str!("../../plugins/documents/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-documents",
        language: "zh-Hans",
        source: include_str!("../../plugins/documents/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-edit-as-buffer",
        language: "fr",
        source: include_str!("../../plugins/edit-as-buffer/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-edit-as-buffer",
        language: "it",
        source: include_str!("../../plugins/edit-as-buffer/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-edit-as-buffer",
        language: "zh-Hans",
        source: include_str!("../../plugins/edit-as-buffer/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-email",
        language: "fr",
        source: include_str!("../../plugins/email/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-email",
        language: "it",
        source: include_str!("../../plugins/email/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-email",
        language: "zh-Hans",
        source: include_str!("../../plugins/email/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-fonts",
        language: "fr",
        source: include_str!("../../plugins/fonts/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-fonts",
        language: "it",
        source: include_str!("../../plugins/fonts/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-fonts",
        language: "zh-Hans",
        source: include_str!("../../plugins/fonts/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-geo",
        language: "fr",
        source: include_str!("../../plugins/geo/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-geo",
        language: "it",
        source: include_str!("../../plugins/geo/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-geo",
        language: "zh-Hans",
        source: include_str!("../../plugins/geo/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-git",
        language: "fr",
        source: include_str!("../../plugins/git/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-git",
        language: "it",
        source: include_str!("../../plugins/git/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-git",
        language: "zh-Hans",
        source: include_str!("../../plugins/git/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-hex",
        language: "fr",
        source: include_str!("../../plugins/hex/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-hex",
        language: "it",
        source: include_str!("../../plugins/hex/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-hex",
        language: "zh-Hans",
        source: include_str!("../../plugins/hex/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-jump",
        language: "fr",
        source: include_str!("../../plugins/jump/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-jump",
        language: "it",
        source: include_str!("../../plugins/jump/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-jump",
        language: "zh-Hans",
        source: include_str!("../../plugins/jump/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-map-tiles",
        language: "fr",
        source: include_str!("../../plugins/map-tiles/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-map-tiles",
        language: "it",
        source: include_str!("../../plugins/map-tiles/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-map-tiles",
        language: "zh-Hans",
        source: include_str!("../../plugins/map-tiles/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-markdown",
        language: "fr",
        source: include_str!("../../plugins/markdown/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-markdown",
        language: "it",
        source: include_str!("../../plugins/markdown/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-markdown",
        language: "zh-Hans",
        source: include_str!("../../plugins/markdown/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-media",
        language: "fr",
        source: include_str!("../../plugins/media/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-media",
        language: "it",
        source: include_str!("../../plugins/media/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-media",
        language: "zh-Hans",
        source: include_str!("../../plugins/media/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-models",
        language: "fr",
        source: include_str!("../../plugins/models/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-models",
        language: "it",
        source: include_str!("../../plugins/models/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-models",
        language: "zh-Hans",
        source: include_str!("../../plugins/models/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-mpris",
        language: "fr",
        source: include_str!("../../plugins/mpris/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-mpris",
        language: "it",
        source: include_str!("../../plugins/mpris/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-mpris",
        language: "zh-Hans",
        source: include_str!("../../plugins/mpris/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-name-tags",
        language: "fr",
        source: include_str!("../../plugins/name-tags/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-name-tags",
        language: "it",
        source: include_str!("../../plugins/name-tags/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-name-tags",
        language: "zh-Hans",
        source: include_str!("../../plugins/name-tags/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-open-with",
        language: "fr",
        source: include_str!("../../plugins/open-with/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-open-with",
        language: "it",
        source: include_str!("../../plugins/open-with/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-open-with",
        language: "zh-Hans",
        source: include_str!("../../plugins/open-with/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-ops-basic",
        language: "fr",
        source: include_str!("../../plugins/ops-basic/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-ops-basic",
        language: "it",
        source: include_str!("../../plugins/ops-basic/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-ops-basic",
        language: "zh-Hans",
        source: include_str!("../../plugins/ops-basic/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-places",
        language: "fr",
        source: include_str!("../../plugins/places/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-places",
        language: "it",
        source: include_str!("../../plugins/places/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-places",
        language: "zh-Hans",
        source: include_str!("../../plugins/places/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-playlists",
        language: "fr",
        source: include_str!("../../plugins/playlists/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-playlists",
        language: "it",
        source: include_str!("../../plugins/playlists/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-playlists",
        language: "zh-Hans",
        source: include_str!("../../plugins/playlists/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-print",
        language: "fr",
        source: include_str!("../../plugins/print/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-print",
        language: "it",
        source: include_str!("../../plugins/print/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-print",
        language: "zh-Hans",
        source: include_str!("../../plugins/print/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-query",
        language: "fr",
        source: include_str!("../../plugins/query/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-query",
        language: "it",
        source: include_str!("../../plugins/query/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-query",
        language: "zh-Hans",
        source: include_str!("../../plugins/query/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-remote",
        language: "fr",
        source: include_str!("../../plugins/remote/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-remote",
        language: "it",
        source: include_str!("../../plugins/remote/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-remote",
        language: "zh-Hans",
        source: include_str!("../../plugins/remote/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-rename-bulk",
        language: "fr",
        source: include_str!("../../plugins/rename-bulk/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-rename-bulk",
        language: "it",
        source: include_str!("../../plugins/rename-bulk/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-rename-bulk",
        language: "zh-Hans",
        source: include_str!("../../plugins/rename-bulk/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shelf",
        language: "fr",
        source: include_str!("../../plugins/shelf/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shelf",
        language: "it",
        source: include_str!("../../plugins/shelf/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shelf",
        language: "zh-Hans",
        source: include_str!("../../plugins/shelf/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shell",
        language: "fr",
        source: include_str!("../../plugins/shell/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shell",
        language: "it",
        source: include_str!("../../plugins/shell/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shell",
        language: "zh-Hans",
        source: include_str!("../../plugins/shell/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shrink",
        language: "fr",
        source: include_str!("../../plugins/shrink/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shrink",
        language: "it",
        source: include_str!("../../plugins/shrink/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-shrink",
        language: "zh-Hans",
        source: include_str!("../../plugins/shrink/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-skwird",
        language: "fr",
        source: include_str!("../../plugins/skwird/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-skwird",
        language: "it",
        source: include_str!("../../plugins/skwird/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-skwird",
        language: "zh-Hans",
        source: include_str!("../../plugins/skwird/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-structured-data",
        language: "fr",
        source: include_str!("../../plugins/structured-data/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-structured-data",
        language: "it",
        source: include_str!("../../plugins/structured-data/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-structured-data",
        language: "zh-Hans",
        source: include_str!("../../plugins/structured-data/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tables",
        language: "fr",
        source: include_str!("../../plugins/tables/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tables",
        language: "it",
        source: include_str!("../../plugins/tables/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tables",
        language: "zh-Hans",
        source: include_str!("../../plugins/tables/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tags",
        language: "fr",
        source: include_str!("../../plugins/tags/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tags",
        language: "it",
        source: include_str!("../../plugins/tags/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tags",
        language: "zh-Hans",
        source: include_str!("../../plugins/tags/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tagspaces",
        language: "fr",
        source: include_str!("../../plugins/tagspaces/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tagspaces",
        language: "it",
        source: include_str!("../../plugins/tagspaces/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-tagspaces",
        language: "zh-Hans",
        source: include_str!("../../plugins/tagspaces/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-terminal",
        language: "fr",
        source: include_str!("../../plugins/terminal/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-terminal",
        language: "it",
        source: include_str!("../../plugins/terminal/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-terminal",
        language: "zh-Hans",
        source: include_str!("../../plugins/terminal/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-thumbnails",
        language: "fr",
        source: include_str!("../../plugins/thumbnails/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-thumbnails",
        language: "it",
        source: include_str!("../../plugins/thumbnails/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-thumbnails",
        language: "zh-Hans",
        source: include_str!("../../plugins/thumbnails/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-timeline",
        language: "fr",
        source: include_str!("../../plugins/timeline/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-timeline",
        language: "it",
        source: include_str!("../../plugins/timeline/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-timeline",
        language: "zh-Hans",
        source: include_str!("../../plugins/timeline/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-trash",
        language: "fr",
        source: include_str!("../../plugins/trash/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-trash",
        language: "it",
        source: include_str!("../../plugins/trash/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-trash",
        language: "zh-Hans",
        source: include_str!("../../plugins/trash/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-updates",
        language: "fr",
        source: include_str!("../../plugins/updates/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-updates",
        language: "it",
        source: include_str!("../../plugins/updates/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-updates",
        language: "zh-Hans",
        source: include_str!("../../plugins/updates/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-usage",
        language: "fr",
        source: include_str!("../../plugins/usage/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-usage",
        language: "it",
        source: include_str!("../../plugins/usage/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-usage",
        language: "zh-Hans",
        source: include_str!("../../plugins/usage/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-vim",
        language: "fr",
        source: include_str!("../../plugins/vim/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-vim",
        language: "it",
        source: include_str!("../../plugins/vim/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-vim",
        language: "zh-Hans",
        source: include_str!("../../plugins/vim/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-xmp",
        language: "fr",
        source: include_str!("../../plugins/xmp/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-xmp",
        language: "it",
        source: include_str!("../../plugins/xmp/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-plugin-xmp",
        language: "zh-Hans",
        source: include_str!("../../plugins/xmp/l10n/zh-Hans.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-ui",
        language: "fr",
        source: include_str!("../../ui/l10n/fr.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-ui",
        language: "it",
        source: include_str!("../../ui/l10n/it.ftl"),
    },
    skwir_core::words::Shipped {
        domain: "skwir-ui",
        language: "zh-Hans",
        source: include_str!("../../ui/l10n/zh-Hans.ftl"),
    },
];

/// Of all the messages, how many each language's catalogs translate and
/// how many of those are reviewed, whether Skwir offers it, as a draft
/// or reviewed (`tools/words/offered`), and its name in itself.
#[rustfmt::skip]
pub static SHARES: &[skwir_core::words::Share] = &[
    skwir_core::words::Share {
        language: "fr",
        messages: 7_100,
        translated: 7_100,
        reviewed: 0,
        offered: skwir_core::words::Offer::Draft,
        name: "Français",
    },
    skwir_core::words::Share {
        language: "it",
        messages: 7_100,
        translated: 7_100,
        reviewed: 0,
        offered: skwir_core::words::Offer::Draft,
        name: "Italiano",
    },
    skwir_core::words::Share {
        language: "zh-Hans",
        messages: 7_100,
        translated: 7_100,
        reviewed: 0,
        offered: skwir_core::words::Offer::Draft,
        name: "简体中文",
    },
];
