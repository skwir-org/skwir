# Italian catalog of skwir-plugin-edit-as-buffer, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
edit_as_buffer_command = Modifica come testo…

# draft
edit_as_buffer_command_description = Apre l’elenco in $EDITOR; il salvataggio propone un piano da esaminare

# draft
edit_as_buffer_duplicate_handle = riga { $line }: l’identificativo { $handle } compare due volte

# draft
edit_as_buffer_duplicate_target = `{ $name }` è la destinazione di più righe

# draft
edit_as_buffer_editor_exited = l’editor è terminato con { $status }

# draft
edit_as_buffer_editor_failed = l’editor non è riuscito

# draft
edit_as_buffer_empty_name = un nome non può essere vuoto

# draft
edit_as_buffer_failed = modifica come testo: { $error }

# draft
edit_as_buffer_header = # Skwir: modifica questo elenco, salva ed esci.
    #   cambia un nome                -> rinomina
    #   elimina una riga              -> sposta nel cestino
    #   nome con una barra            -> sposta in quella cartella (creata se serve)
    #   nuova riga che termina con /  -> nuova cartella
    # Mantieni il numero all’inizio di ogni riga. L’ordine non conta.
    # Nei nomi, \\ è una barra rovesciata e \xHH un byte grezzo (\x0a un a capo,
    # \x20 uno spazio a un’estremità).

# draft
edit_as_buffer_invalid_name = riga { $line }: nome non valido `{ $name }`: { $reason }

# draft
edit_as_buffer_menu = Modifica come testo…

# draft
edit_as_buffer_new_file = riga { $line }: i nuovi file non sono supportati (`{ $name }`); termina la riga con / per una cartella

# draft
edit_as_buffer_no_changes = modifica come testo: nessuna modifica

# draft
edit_as_buffer_no_pane = riquadro inesistente

# draft
edit_as_buffer_nothing_open = il riquadro non ha alcuna posizione aperta

# draft
edit_as_buffer_outside = un percorso deve restare all’interno di questa cartella

# draft
edit_as_buffer_plan = modifica { $folder }

# draft
edit_as_buffer_plugin_description = Modifica l’elenco nel tuo editor; la differenza diventa un piano da esaminare

# draft
edit_as_buffer_plugin_title = Modifica come testo

# draft
edit_as_buffer_taken = riga { $line }: `{ $name }` esiste già

# draft
edit_as_buffer_unknown_handle = riga { $line }: identificativo { $handle } sconosciuto

# draft
edit_as_buffer_unreadable = impossibile rileggere il buffer modificato
