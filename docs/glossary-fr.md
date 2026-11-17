# French glossary (proposal, to review)

Status: drafted 2026-11-17 for ADR-139's step G, **not yet settled with the
user**. Every `fr.ftl` is drafted from it, each message marked `# draft`
until reviewed. A term changed here is changed in the drafts by search.

The terms follow what French-speaking users already read in their file
managers (GNOME Fichiers, Dolphin, the Windows Explorer, the macOS
Finder), choosing the most common where they differ. A choice still open
is marked **(?)**.

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
| tag | étiquette | Dolphin; the Finder says « tag » **(?)** |
| rating | note | « 4 étoiles » for its stars |
| comment | commentaire | |
| notes (tags, rating, comment) | annotations **(?)** | Skwir's word for the three together |
| Properties | Propriétés | |
| preview | aperçu | |
| quick preview | aperçu rapide | |
| thumbnail | vignette | |
| sidebar | barre latérale | |
| place (sidebar) | emplacement | |
| location | emplacement | |
| address (field) | adresse | « barre d’adresse » for the field |
| trail (the path's crumbs) | fil d’Ariane | |
| tab | onglet | |
| pane | panneau | |
| split | diviser la vue | |
| view: list, grid, columns, board | vue : liste, grille, colonnes, tableau | |
| sort by / group by | trier par / regrouper par | |
| folders first | dossiers en premier | |
| name, size, modified, created, accessed | nom, taille, modifié, créé, consulté | as column headers: Nom, Taille, Modifié le **(?)** |
| type (a file's) | type | the MIME type's description |
| kind (what a type is: image, video…) | genre **(?)** | the Finder says « Type » for kind, Dolphin « Catégorie » |
| search / filter | recherche / filtre | |
| saved search | recherche enregistrée | |
| Quick filters | filtres rapides | |
| Shelf | Étagère | Skwir's place for what waits to be pasted **(?)** |
| bookmark | signet | GNOME; Dolphin says « marque-page » |
| Recent | Récents | |
| Home | Dossier personnel | GNOME and Dolphin |
| Nearby | À proximité | |
| remote folder / server | dossier distant / serveur | |
| connect / disconnect | se connecter / se déconnecter | |
| mount / unmount, eject | monter / démonter, éjecter | |
| device, volume, drive | périphérique, volume, disque | |
| plan (changes Skwir proposes) | plan | « le plan » of changes reviewed before applying **(?)** |
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
| plugin | module **(?)** | « extension » in GNOME and browsers, « module externe » in Dolphin |
| agent | agent | an AI or a script that drives Skwir |
| Usage (disk usage view) | Occupation | |
| extract / compress | extraire / compresser | |
| archive | archive | |
| Make Smaller Copy | Faire une copie allégée **(?)** | |
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
