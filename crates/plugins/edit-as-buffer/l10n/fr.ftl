# French catalog of skwir-plugin-edit-as-buffer, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
edit_as_buffer_command = Modifier comme texte…

# draft
edit_as_buffer_command_description = Ouvrir la liste du dossier dans $EDITOR ; l’enregistrer propose un plan à examiner

# draft
edit_as_buffer_duplicate_handle = ligne { $line } : le numéro { $handle } apparaît deux fois

# draft
edit_as_buffer_duplicate_target = `{ $name }` est la cible de plusieurs lignes

# draft
edit_as_buffer_editor_exited = l’éditeur s’est terminé avec { $status }

# draft
edit_as_buffer_editor_failed = échec de l’éditeur

# draft
edit_as_buffer_empty_name = un nom ne peut pas être vide

# draft
edit_as_buffer_failed = modifier comme texte : { $error }

# draft
edit_as_buffer_header = # skwir : modifiez cette liste, enregistrez et quittez.
    #   changer un nom                -> renommer
    #   supprimer une ligne           -> mettre à la corbeille
    #   nom avec une barre oblique    -> déplacer dans ce dossier (créé au besoin)
    #   nouvelle ligne finie par /    -> nouveau dossier
    # Gardez le numéro au début de chaque ligne. L’ordre est sans importance.
    # Dans les noms, \\ est une barre oblique inverse et \xHH un octet brut
    # (\x0a un saut de ligne, \x20 un espace en début ou en fin de nom).

# draft
edit_as_buffer_invalid_name = ligne { $line } : nom non valide `{ $name }` : { $reason }

# draft
edit_as_buffer_menu = Modifier comme texte…

# draft
edit_as_buffer_new_file = ligne { $line } : les nouveaux fichiers ne sont pas pris en charge (`{ $name }`) ; terminez la ligne par / pour un dossier

# draft
edit_as_buffer_no_changes = modifier comme texte : aucun changement

# draft
edit_as_buffer_no_pane = panneau introuvable

# draft
edit_as_buffer_nothing_open = le panneau n’a aucun emplacement ouvert

# draft
edit_as_buffer_outside = un chemin doit rester dans ce dossier

# draft
edit_as_buffer_plan = modifier { $folder }

# draft
edit_as_buffer_plugin_description = Modifier la liste du dossier dans votre éditeur ; la différence devient un plan à examiner

# draft
edit_as_buffer_plugin_title = Édition en texte

# draft
edit_as_buffer_taken = ligne { $line } : `{ $name }` existe déjà

# draft
edit_as_buffer_unknown_handle = ligne { $line } : numéro inconnu { $handle }

# draft
edit_as_buffer_unreadable = impossible de relire le texte modifié
