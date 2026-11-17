# French catalog of skwir-plugin-shell, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
shell_ask_accept = Exécuter

# draft
shell_ask_run = Exécuter une commande sur la sélection

# draft
shell_ask_run_here = Exécuter une commande dans ce dossier

# draft
shell_ask_run_here_placeholder = Commande ; elle s’exécute dans ce dossier

# draft
shell_ask_run_placeholder = Commande ; les chemins sélectionnés sont ses arguments

# draft
shell_copied_paths = { $count ->
        [one] { $count } chemin copié
       *[other] { $count } chemins copiés
    }

# draft
shell_copy_paths = Copier les chemins pour un shell

# draft
shell_copy_paths_description = Les chemins sélectionnés, ou ceux des éléments donnés, protégés pour le shell, dans le presse-papiers

# draft
shell_example_df = Le remplissage de son disque

# draft
shell_example_du = La place que prend chacun

# draft
shell_example_du_here = La place qu’il prend

# draft
shell_example_file = Ce qu’est chacun

# draft
shell_example_git = Ce qu’en dit git

# draft
shell_example_ls = Ce qu’il contient, fichiers cachés compris

# draft
shell_example_ls_ld = Leurs détails

# draft
shell_example_sha = Leurs sommes de contrôle

# draft
shell_example_wc = Le nombre de lignes de chacun

# draft
shell_examples = Par exemple

# draft
shell_line_more = { $line } … et { $more ->
        [one] { $more } autre chemin
       *[other] { $more } autres chemins
    }

# draft
shell_line_named = { $command }  avec les chemins là où elle les nomme : { $paths }

# draft
shell_line_no_paths = { $command }  sans chemin

# draft
shell_no_folder = les éléments ne sont dans aucun dossier

# draft
shell_plugin_description = Copier les chemins sélectionnés pour un shell ; exécuter une commande sur la sélection dans un terminal

# draft
shell_plugin_title = Shell

# draft
shell_program = Programme des commandes

# draft
shell_program_description = Le shell qui exécute une commande sur la sélection : un shell POSIX ou fish.

# draft
shell_program_description_more = Laissez vide pour le vôtre ($SHELL) s’il est l’un d’eux, sinon sh.

# draft
shell_reference_title = Commande

# draft
shell_run = Exécuter une commande sur la sélection…

# draft
shell_run_describe = Dire ce qu’exécute une commande

# draft
shell_run_describe_description = Dire ce que `shell.run` exécuterait avec `command` sur les chemins sélectionnés, ou ceux des éléments donnés : la ligne avec les premiers chemins tels que son shell les reçoit (après elle, ou là où elle nomme ses arguments), combien d’autres, et le dossier ; une erreur pour une commande vide, ou là où rien ne s’exécute

# draft
shell_run_description = Exécuter une commande shell dans un terminal, avec comme arguments les chemins sélectionnés, ou ceux des éléments donnés

# draft
shell_run_here = Exécuter une commande ici…

# draft
shell_run_here_describe = Dire ce qu’exécute une commande ici

# draft
shell_run_here_describe_description = Dire ce que `shell.run_here` exécuterait avec `command` : la ligne et le dossier ; une erreur pour une commande vide, ou là où rien ne s’exécute

# draft
shell_run_here_description = Exécuter une commande shell dans un terminal, dans un dossier (`in`, sinon celui qu’affiche le panneau), sans chemin

# draft
shell_run_menu = Exécuter une commande…

# draft
shell_run_not_yet = Exécuter une commande dans un terminal n’est pas encore disponible sous { $system }

# draft
shell_runs = Exécute { $line }  dans { $folder }

# draft
shell_runs_here_note = Elle s’exécute dans un terminal, dans ce dossier, sans chemin après elle, avec le shell que nomme Paramètres › Terminal, et le terminal reste ouvert ensuite.

# draft
shell_runs_note = Les chemins sélectionnés viennent après elle, chacun un argument, sauf si elle les nomme elle-même ($@ entre guillemets ou $1… en sh, bash ou zsh, $argv en fish) : ils ne vont alors que là. Elle s’exécute dans un terminal, dans le dossier qui les contient, avec le shell que nomme Paramètres › Terminal, et le terminal reste ouvert ensuite.

# draft
shell_write_command = Écrivez la commande à exécuter : les chemins sélectionnés sont ses arguments

# draft
shell_write_command_here = Écrivez la commande à exécuter dans ce dossier
