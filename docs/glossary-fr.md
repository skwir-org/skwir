# French glossary

Status: drafted 2026-11-17 for ADR-139's step G and **settled with the
user on 2026-11-17**. Every `fr.ftl` is drafted from it, each message
marked `# draft` until reviewed. A term changed here is changed in the
drafts by search.

The terms follow what French-speaking users already read in their file
managers (GNOME Fichiers, Dolphin, the Windows Explorer, the macOS
Finder), choosing the most common where they differ. Where an English
word has two senses (a pane and a panel, Skwir's notes and a document's,
a file's kind and a book's genre), each sense has its own French word:
decide by the English of the same id and its note for translators.

## Writing

- **The reader is *vous*.** Instructions say what to do, not what "the
  user" does.
- **Buttons and menu items are infinitives:** « Copier », « Renommer… »,
  « Vider la corbeille… ». Toasts and notices say what was done, in the
  passé composé: « 3 éléments copiés dans Documents ».
- **Sentence case** everywhere: only the first word and proper names take
  a capital (« Tri par date de modification », not « Tri Par Date »).
- **Ellipsis:** « … » (U+2026) after a command that asks something
  before it acts, as in English.
- **Apostrophe:** ’ (U+2019), never '.

## Typography

- **Quotes:** « name », with a narrow no-break space (U+202F) inside each
  guillemet. Skwir adds them itself around a `Name` argument: never write
  quotes around `{ $name }` in a message.
- **Before `; ! ?` and `»`:** a narrow no-break space (U+202F). **Before
  `:`:** a no-break space (U+00A0) (Imprimerie nationale). After `«`, a
  narrow no-break space.
- **Numbers:** digits grouped by a narrow no-break space (U+202F), a
  decimal comma: « 1 234,5 ». Skwir formats counts and sizes itself
  (`Count`, `Size` arguments): write `{ $count }`, never a number.
- **Units:** ko, Mo, Go, To (1000-based, the default); Kio, Mio, Gio,
  Tio (1024-based). A no-break space between a number and its unit
  (« 1,5 Mo »).
- **Plurals:** French says 0 and 1 in the singular (CLDR's `one` covers
  both): every selector on a count has `[one]` and `*[other]`. Where the
  English chooses by the exact `[1]` (one thing named, several counted),
  keep `[1]` for the named one and add `[one]` for 0: « 0 élément ».
  `[many]` (a round million: « 1 000 000 de fichiers ») is optional.

## Places in sentences

Skwir's places are nouns (`# noun` above them in `en.ftl`): each French
one is given its gender and two forms (`tools/words/grammar`'s `fr`
line, drafted 2026-11-18), which a sentence naming a place reads.

- **`.definite`**, the place with its article: « dans { FORM($place,
  case: "definite") } » is « dans la Corbeille », and « dans Docs » for
  a folder the user named, which has no article.
- **`.de`**, the place after « de », its article fused (« du », « de
  la », « de l’ »), which a name has not: write it inside a `GENDER`
  selector, the name's « de » in `*[other]`:

  ```
  loading_opening = Ouverture { GENDER($place) ->
          [masculine] { FORM($place, case: "de") }
          [feminine] { FORM($place, case: "de") }
         *[other] de { $place }
      }…
  ```
- **`.gender`**, `masculine` or `feminine`, for what agrees with the
  place (« la Corbeille est vide », « le Dossier personnel est vide »);
  a name is `other`, whose words agree with neither (« Docs est vide »).
- A place keeps its capital inside a sentence, as in a title (« dans la
  Corbeille »). One that takes no article in prose (Récents, À
  proximité) is its own `.definite`.

| Place | `.gender` | `.definite` | `.de` |
| --- | --- | --- | --- |
| Home | masculine | le Dossier personnel | du Dossier personnel |
| Trash | feminine | la Corbeille | de la Corbeille |
| Recent | masculine | Récents | de Récents |
| Shelf | feminine | l’Étagère | de l’Étagère |
| Nearby | masculine | À proximité | d’À proximité |
| Root (the system's volume) | feminine | la Racine | de la Racine |

## Terms

| English | French | Note |
| --- | --- | --- |
| file | fichier | |
| folder | dossier | |
| item (file or folder) | élément | |
| entry | élément | an entry in a listing |
| link (symbolic) | lien | « lien symbolique » where it must be said whole |
| Trash | Corbeille | a place: capital, no article in a title |
| move to the trash | mettre à la corbeille | |
| empty the trash | vider la corbeille | |
| restore | restaurer | |
| delete (for good) | supprimer (définitivement) | |
| copy / cut / paste | copier / couper / coller | |
| move | déplacer | |
| rename | renommer | |
| undo / redo | annuler / rétablir | |
| new folder | nouveau dossier | |
| open / open with | ouvrir / ouvrir avec | |
| hidden files | fichiers cachés | |
| tag | étiquette | Dolphin, Thunderbird, the OQLF. The query language's `tag:` stays as written; TagSpaces' own words (its name, its file format) stay |
| rating | note | « 4 étoiles » for its stars; « noter », « non noté » |
| comment | commentaire | |
| notes (tags, rating, comment) | annotations | Skwir's word for the three together: « note » is a rating in French |
| a document's own notes (speaker notes, footnotes, a book's notes) | notes | « les notes du document » |
| Properties | Propriétés | |
| preview | aperçu | |
| quick preview | aperçu rapide | |
| viewer (`skwir view`'s window) | visionneuse | « visionneuse de fichiers » for its kind |
| thumbnail | vignette | |
| sidebar | barre latérale | |
| place (sidebar) | emplacement | |
| location | emplacement | |
| address (field) | adresse | « barre d’adresse » for the field |
| trail (the path's crumbs) | fil d’Ariane | |
| tab | onglet | |
| pane | panneau | one of the panes showing folders, side by side when split |
| panel (the preview's, the terminal's, a side panel) | volet | « le volet d’aperçu », « le volet du terminal »; never « panneau », which is a pane |
| split | diviser la vue | |
| view: list, grid, columns, board | vue : liste, grille, colonnes, tableau | |
| sort by / group by | trier par / regrouper par | |
| folders first | dossiers en premier | |
| name, size, modified, created, accessed | nom, taille, modifié, créé, consulté | as column headers: Nom, Taille, Modifié le |
| type (a file's) | type | its type name, the MIME type's description (« Image PNG ») |
| kind (what a type is: image, video, document…; `mime::Kind`) | genre | « Genre », « du même genre »; `kind:` stays as written |
| a book's literary genre | genre littéraire | « Genre littéraire » in a book's Properties, never confused with a file's kind |
| search / filter | recherche / filtre | |
| saved search | recherche enregistrée | |
| Quick filters | filtres rapides | |
| index (of names, that answers searches) | index (m.), index des noms | « l’index de skwird »; the settings page is « Index »; drafted 2026-11-23 |
| keep live, kept live | suivre en direct, suivi en direct | the Index page's list is « Suivis en direct »; the menu's entry « Suivre en direct pour les recherches »; drafted 2026-11-23 |
| live / not live (a folder's state) | en direct / pas en direct | drafted 2026-11-23 |
| up to date (an index) | à jour | the footer's « index à jour »; an index as of a moment « index mis à jour : … »; drafted 2026-11-23 |
| checked (an index compared with its folders) | vérifié | the footer's « index vérifié : … »; drafted 2026-11-23 |
| watch (what the system watches a folder with) | surveillance | « les surveillances du système »; drafted 2026-11-23 |
| share (of the watches skwird may use) | part | « la part de surveillances »; drafted 2026-11-23 |
| engine (skwird's) | moteur | « un moteur de skwird », as a reader's engine is a « moteur »; drafted 2026-11-23 |
| system service | service système | « le service système de skwird »; drafted 2026-11-23 |
| Your folders (the desktop's: Documents, Pictures…) | Vos dossiers | a group of the folders kept live; drafted 2026-11-23 |
| Added by you | Ajoutés par vous | agrees with what it heads: « Ajoutée par vous » on an exclusion; drafted 2026-11-23 |
| tick / untick (a row's box) | cocher / décocher | drafted 2026-11-23 |
| Shelf | Étagère | Skwir's place for what waits to be pasted; a place's name: capital É, « l’Étagère », « sur l’Étagère » |
| bookmark | signet | GNOME; Dolphin says « marque-page » |
| Recent | Récents | |
| Home | Dossier personnel | GNOME and Dolphin |
| Nearby | À proximité | |
| remote folder / server | dossier distant / serveur | |
| connect / disconnect | se connecter / se déconnecter | |
| mount / unmount, eject | monter / démonter, éjecter | |
| device, volume, drive | périphérique, volume, disque | |
| plan (changes Skwir proposes) | plan | « le plan » of changes reviewed before they apply |
| proposal / review | proposition / revue | « examiner » as a verb |
| apply | appliquer | |
| step | étape | |
| Activity | Activité | |
| History | Historique | |
| Earlier | Plus tôt | Activity's section of what ended |
| cancel / stop | annuler / arrêter | |
| conflict, name taken | conflit, nom déjà pris | |
| replace / keep both / skip | remplacer / garder les deux / ignorer | |
| merge (folders) | fusionner | |
| command palette | palette de commandes | |
| shortcut, key | raccourci, touche | |
| Settings | Paramètres | GNOME; the Finder says « Réglages » |
| exclusions | exclusions | |
| plugin | module | never « extension » (a file name's extension) nor « greffon », but another program's own plugins keep the French their program gives them (« greffons GStreamer », as GNOME writes them) |
| plugin from outside (ADR-045) | module externe | |
| WebAssembly module | module WebAssembly | |
| agent | agent | an AI or a script that drives Skwir |
| Skwir (the product, in prose) | Skwir | « skwir » where it names the program a terminal runs (`skwir --probe`), a path or an id, and where the product shows its name as a brand: the window's title (« Téléchargements — skwir ») and the launcher's name, lowercase by the user's choice (2026-11-17) |
| Usage (disk usage view) | Occupation | |
| extract / compress | extraire / compresser | |
| archive | archive | |
| Make Smaller Copy | Faire une copie allégée | |
| print / save as PDF | imprimer / enregistrer en PDF | |
| page, booklet | page, livret | |
| player, track, playlist | lecteur, piste, liste de lecture | |
| volume (sound) | volume | |
| fullscreen | plein écran | |
| zoom in / out | zoomer / dézoomer | |
| terminal | terminal | |
| map | carte | |
| Ctrl, Shift, Alt, Super, Esc, Enter, Tab, Space, Backspace, Delete | Ctrl, Maj, Alt, Super, Échap, Entrée, Tab, Espace, Retour arrière, Suppr | as GNOME and Windows show them in French |
| Page Up / Page Down, Home / End | Page préc. / Page suiv., Début / Fin | |
