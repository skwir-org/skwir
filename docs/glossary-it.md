# Italian glossary

Status: drafted 2026-11-21. Every `it.ftl` is drafted from it, each
message marked `# draft` until a person who speaks Italian reviews it. A
term changed here is changed in the drafts by search.

The terms follow what Italian-speaking users already read in their file
managers (GNOME File, Dolphin, Esplora file di Windows, il Finder di
macOS), choosing the most common where they differ. Where an English
word has two senses (a pane and a panel, Skwir's notes and a document's,
a file's kind and a book's genre), each sense has its own Italian word:
decide by the English of the same id and its note for translators.

## Writing

- **Buttons and menu items are imperatives, second person singular**
  (« Copia », « Rinomina… », « Svuota cestino… »), as GNOME File,
  Dolphin and Windows show them. Instructions address the reader the
  same way (« Scegli una cartella »), never « l’utente ».
- **Toasts and notices say what was done**, with a past participle:
  « 3 elementi copiati in Documenti ».
- **Sentence case** everywhere: only the first word and proper names take
  a capital (« Ordina per data di modifica »).
- **Ellipsis:** « … » (U+2026) after a command that asks something
  before it acts, as in English.
- **Apostrophe:** ’ (U+2019), never ' (« l’elemento », « un’immagine »).

## Typography

- **Quotes:** « name » (guillemets, no space inside). Skwir adds them
  itself around a `Name` argument: never write quotes around
  `{ $name }` in a message.
- **No space before `: ; ! ?`**, as in English.
- **Numbers:** Skwir formats counts and sizes itself (`Count`, `Size`
  arguments): write `{ $count }`, never a number.
- **Units:** kB, MB, GB, TB (1000-based, the default); KiB, MiB, GiB,
  TiB (1024-based). A no-break space between a number and its unit
  (`{ $n }\u{a0}MB`), as the English writes it.
- **Percent:** no space (« 42% »), as the English.
- **Plurals:** Italian has `[one]` (exactly 1) and `*[other]`; CLDR also
  gives `[many]` for round millions (« 1 milione di file »), optional.
  Where the English chooses by the exact `[1]` (one thing named, several
  counted), keep `[1]`.

## Places in sentences

Skwir's places are nouns (`# noun` above them in `en.ftl`): each Italian
one is given its gender and four forms (`tools/words/grammar`'s `it`
line), which a sentence naming a place reads.

- **`.definite`**, the place with its article: « il Cestino ».
- **`.di`, `.in`, `.da`**, the place after the preposition, its article
  fused (« del Cestino », « nel Cestino », « dal Cestino »), which a
  name has not: write them inside a `GENDER` selector, what a name takes
  in `*[other]`:

  ```
  loading_opening = Apertura { GENDER($place) ->
          [masculine] { FORM($place, case: "di") }
          [feminine] { FORM($place, case: "di") }
         *[other] di { $place }
      }…
  ```
- **`.gender`**, `masculine` or `feminine`, for what agrees with the
  place (« il Cestino è vuoto », « la Mensola è vuota »); a name is
  `other`: write the masculine agreement there.
- A place keeps its capital inside a sentence (« nel Cestino »). One
  that takes no article in prose (Recenti) is its own `.definite`.

| Place | `.gender` | `.definite` | `.di` | `.in` | `.da` |
| --- | --- | --- | --- | --- | --- |
| Home | feminine | la Cartella personale | della Cartella personale | nella Cartella personale | dalla Cartella personale |
| Trash | masculine | il Cestino | del Cestino | nel Cestino | dal Cestino |
| Recent | masculine | Recenti | di Recenti | in Recenti | da Recenti |
| Shelf | feminine | la Mensola | della Mensola | nella Mensola | dalla Mensola |
| Nearby | masculine | i Dispositivi vicini | dei Dispositivi vicini | nei Dispositivi vicini | dai Dispositivi vicini |
| Root (the system's volume) | feminine | la Radice | della Radice | nella Radice | dalla Radice |

## Terms

| English | Italian | Note |
| --- | --- | --- |
| file | file | invariable: « 1 file », « 3 file » |
| folder | cartella | |
| item (file or folder) | elemento | |
| entry | voce | an entry in a listing or a menu |
| link (symbolic) | collegamento | « collegamento simbolico » where it must be said whole |
| Trash | Cestino | a place: capital |
| move to the trash | sposta nel cestino | |
| empty the trash | svuota il cestino | |
| restore | ripristina | |
| delete (for good) | elimina (definitivamente) | |
| copy / cut / paste | copia / taglia / incolla | |
| move | sposta | |
| rename | rinomina | |
| undo / redo | annulla / ripeti | |
| new folder | nuova cartella | |
| open / open with | apri / apri con | |
| hidden files | file nascosti | |
| tag | etichetta | the query language's `tag:` stays as written; TagSpaces' own words stay |
| rating | valutazione | « 4 stelle »; « valuta », « non valutato » |
| comment | commento | |
| notes (tags, rating, comment) | annotazioni | Skwir's word for the three together |
| a document's own notes | note | « le note del documento » |
| Properties | Proprietà | |
| preview | anteprima | |
| quick preview | anteprima rapida | |
| thumbnail | miniatura | |
| sidebar | barra laterale | |
| place (sidebar), location | posizione | |
| address (field) | indirizzo | « barra degli indirizzi » for the field |
| trail (the path's crumbs) | percorso | |
| tab | scheda | |
| pane | riquadro | one of the panes showing folders, side by side when split |
| panel (preview's, terminal's, a side panel) | pannello | « pannello di anteprima », never « riquadro » |
| split | dividi la vista | |
| view: list, grid, columns, board | vista: elenco, griglia, colonne, bacheca | |
| sort by / group by | ordina per / raggruppa per | |
| folders first | cartelle per prime | |
| name, size, modified, created, accessed | nome, dimensione, modificato, creato, ultimo accesso | |
| type (a file's) | tipo | |
| kind (`mime::Kind`) | genere | `kind:` stays as written |
| a book's literary genre | genere letterario | |
| search / filter | ricerca / filtro | |
| saved search | ricerca salvata | |
| Quick filters | filtri rapidi | |
| Shelf | Mensola | Skwir's place for what waits to be pasted: « sulla Mensola » |
| bookmark | segnalibro | |
| Recent | Recenti | |
| Home | Cartella personale | GNOME |
| Nearby | Dispositivi vicini | |
| remote folder / server | cartella remota / server | |
| connect / disconnect | connetti / disconnetti | |
| mount / unmount, eject | monta / smonta, espelli | |
| device, volume, drive | dispositivo, volume, unità | |
| plan (changes Skwir proposes) | piano | |
| proposal / review | proposta / revisione | « esamina » as a verb |
| apply | applica | |
| step | passaggio | |
| Activity | Attività | |
| History | Cronologia | |
| Earlier | In precedenza | |
| cancel / stop | annulla / interrompi | |
| conflict, name taken | conflitto, nome già in uso | |
| replace / keep both / skip | sostituisci / mantieni entrambi / salta | |
| merge (folders) | unisci | |
| command palette | tavolozza dei comandi | |
| shortcut, key | scorciatoia, tasto | |
| Settings | Impostazioni | |
| plugin | plugin | invariable; another program's own plugins keep their names |
| plugin from outside (ADR-045) | plugin esterno | |
| agent | agente | an AI or a script that drives Skwir |
| Skwir | Skwir | « skwir » where it names the program, a path, an id, the window's title and the launcher's name |
| Usage (disk usage view) | Occupazione | |
| extract / compress | estrai / comprimi | |
| archive | archivio | |
| Make Smaller Copy | Crea copia ridotta | |
| print / save as PDF | stampa / salva come PDF | |
| page, booklet | pagina, opuscolo | |
| player, track, playlist | lettore, traccia, scaletta | |
| fullscreen | schermo intero | |
| zoom in / out | ingrandisci / riduci | |
| map | mappa | |
| Ctrl, Shift, Alt, Super, Esc, Enter, Tab, Space, Backspace, Delete | Ctrl, Maiusc, Alt, Super, Esc, Invio, Tab, Spazio, Backspace, Canc | |
| Page Up / Page Down, Home / End | PagSu / PagGiù, Inizio / Fine | |
