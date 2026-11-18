# French catalog of skwir-extend, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
extend_answer_cut = { $start }…

# draft
extend_answered = { $command } : { $value }

# draft
extend_arguments_object = les arguments doivent être un objet

# draft
extend_command_failed = { $command } : { $error }

# draft
extend_converted_gone = le fichier converti n’est plus là

# draft
extend_copy_answer = Copier

# draft
extend_did_not_start = le module n’a pas démarré

# draft
extend_ending = le module est en train de s’arrêter

# draft
extend_failed_before = le module n’a pas pu démarrer il y a un instant

# draft
extend_file_not_base64 = le fichier du module n’est pas en base64

# draft
extend_install_module_worker = Installez { $program } pour exécuter les modules écrits en WebAssembly

# draft
extend_may_not_write = ce module n’a pas le droit de modifier des fichiers

# draft
extend_no_answer = le module n’a pas répondu à temps

# draft
extend_no_page = Il n’y a pas de page { $page } : le fichier en a { $pages }

# draft
extend_no_such_file = aucun fichier { $handle } dans cet appel

# draft
extend_not_reading = le module ne lit pas ce qui lui est demandé

# draft
extend_page_not_base64 = la page du module n’est pas en base64

# draft
extend_pages_links_closed = Les liens ne sont pas suivis dans les pages dessinées par « { $plugin } » : ce module n’a pas accès au réseau

# draft
extend_plugin_off = le module est désactivé

# draft
extend_preview_links_closed = Les liens ne sont pas suivis dans un aperçu fait par « { $plugin } » : ce module n’a pas accès au réseau

# draft
extend_private_file = un fichier privé : un module externe à Skwir ne le voit pas (Paramètres, Confidentialité)

# draft
extend_private_folder = un dossier privé : aucun module n’y accède

# draft
extend_private_preview = Ce fichier est privé : un module externe à Skwir ne le lit pas (Paramètres, Confidentialité)

# draft
extend_protocol_version = le module utilise la version { $version } du protocole ; Skwir prend en charge les versions { $oldest } à { $newest }

# draft
extend_requirement_module = Exécution du code WebAssembly de ce module

# draft
extend_shows_nothing = « { $plugin } » ne montre rien de ce fichier

# draft
extend_stopped = le module s’est arrêté

# draft
extend_thumbnail_not_base64 = une vignette qui n’est pas en base64

# draft
extend_turned_off = { $count ->
        [one] Le module « { $plugin } » a été désactivé : il ne répondait plus
       *[other] Le module « { $plugin } » a été désactivé : il n’a pas répondu { $count } fois de suite
    }

# draft
extend_turned_off_action = Modules

# draft
extend_turned_off_meanwhile = le module a été désactivé entre-temps
