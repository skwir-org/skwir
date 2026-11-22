# Italian catalog of skwir-plugin-shell, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
shell_ask_accept = Esegui

# draft
shell_ask_run = Esegui comando sulla selezione

# draft
shell_ask_run_here = Esegui comando in questa cartella

# draft
shell_ask_run_here_placeholder = Comando; viene eseguito in questa cartella

# draft
shell_ask_run_placeholder = Comando; i percorsi selezionati sono i suoi argomenti

# draft
shell_copied_paths = { $count ->
        [one] Copiato { $count } percorso
       *[other] Copiati { $count } percorsi
    }

# draft
shell_copy_paths = Copia percorsi per una shell

# draft
shell_copy_paths_description = I percorsi selezionati, o quelli delle voci, protetti per la shell, per gli appunti

# draft
shell_example_df = Quanto è pieno il suo disco

# draft
shell_example_du = Quanto occupa ciascuno

# draft
shell_example_du_here = Quanto occupa

# draft
shell_example_file = Cos’è ciascuno

# draft
shell_example_git = Cosa dice git a riguardo

# draft
shell_example_ls = Cosa contiene, anche i file nascosti

# draft
shell_example_ls_ld = I loro dettagli

# draft
shell_example_sha = I loro checksum

# draft
shell_example_wc = Quante righe ha ciascuno

# draft
shell_examples = Per esempio

# draft
shell_line_more = { $line } … e { $more ->
        [one] { $more } altro percorso
       *[other] altri { $more } percorsi
    }

# draft
shell_line_named = { $command }  con i percorsi dove li nomina: { $paths }

# draft
shell_line_no_paths = { $command }  senza percorsi

# draft
shell_no_folder = le voci non sono in nessuna cartella

# draft
shell_plugin_description = Copia i percorsi selezionati per una shell; esegui un comando sulla selezione in un terminale

# draft
shell_plugin_title = Shell

# draft
shell_program = Programma per i comandi

# draft
shell_program_description = La shell che esegue un comando sulla selezione: una shell POSIX o fish.

# draft
shell_program_description_more = Lascia vuoto per la tua ($SHELL) se è una di queste, altrimenti sh.

# draft
shell_reference_title = Comando

# draft
shell_run = Esegui comando sulla selezione…

# draft
shell_run_describe = Descrivi cosa esegue un comando

# draft
shell_run_describe_description = Descrive cosa eseguirebbe `shell.run` con `command` sui percorsi selezionati, o su quelli delle voci: la riga con i primi percorsi come li riceve la sua shell (dopo di essa, o dove nomina i suoi argomenti), quanti altri, e la cartella; un errore per un comando vuoto, o dove non viene eseguito nulla

# draft
shell_run_description = Esegue un comando di shell in un terminale con i percorsi selezionati, o quelli delle voci, come argomenti

# draft
shell_run_here = Esegui comando qui…

# draft
shell_run_here_describe = Descrivi cosa esegue un comando qui

# draft
shell_run_here_describe_description = Descrive cosa eseguirebbe `shell.run_here` con `command`: la riga e la cartella; un errore per un comando vuoto, o dove non viene eseguito nulla

# draft
shell_run_here_description = Esegue un comando di shell in un terminale in una cartella (`in`, altrimenti quella mostrata dal riquadro), senza percorsi

# draft
shell_run_menu = Esegui comando…

# draft
shell_run_not_yet = L’esecuzione di un comando in un terminale non è ancora disponibile su { $system }

# draft
shell_runs = Esegue { $line }  in { $folder }

# draft
shell_runs_here_note = Viene eseguito in un terminale, in questa cartella, senza percorsi dopo di esso, con la shell indicata da Impostazioni › Terminale; il terminale resta aperto dopo.

# draft
shell_runs_note = I percorsi selezionati vengono dopo di esso, ciascuno un argomento, a meno che non li nomini da sé ("$@" o $1… in sh, bash o zsh, $argv in fish): in tal caso vanno solo lì. Viene eseguito in un terminale, nella cartella che li contiene, con la shell indicata da Impostazioni › Terminale; il terminale resta aperto dopo.

# draft
shell_write_command = Scrivi il comando da eseguire: i percorsi selezionati sono i suoi argomenti

# draft
shell_write_command_here = Scrivi il comando da eseguire in questa cartella
