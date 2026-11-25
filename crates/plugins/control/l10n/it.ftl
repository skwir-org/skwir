# Italian catalog of skwir-plugin-control, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
control_another_listens = un’altra finestra di Skwir è in ascolto

# draft
control_ask = Un agente chiede di eseguire { $command }

# draft
control_ask_detail = { $command }: { $description }

# draft
control_ask_detail_arguments = { $command }: { $description }
    { $arguments }

# draft
control_cannot_listen = Agenti e script non possono mettersi in ascolto: { $error }

# draft
control_channel_folder = la cartella del canale

# draft
control_closed_unasked = Skwir si è chiuso prima che l’utente rispondesse

# draft
control_closed_unreviewed = Skwir si è chiuso prima che il piano ricevesse risposta

# draft
control_closing = Skwir si sta chiudendo

# draft
control_elsewhere = cercare { $path } farebbe raggiungere al sistema un’altra macchina, cosa che un agente non può fare senza permesso: l’utente lo apre prima in Skwir

# draft
control_instructions = Skwir, un file manager. Ogni strumento è uno dei suoi comandi, con i punti scritti come due trattini bassi. Indica i file su cui agisce un comando in `entries`, con percorso assoluto: la selezione è dell’utente e può cambiare tra due chiamate (lo strumento `entries` elenca ciò che mostra un riquadro, o con `location` una cartella, senza cambiare ciò che l’utente vede). Un comando che modificherebbe dei file propone un piano che l’utente esamina nella finestra di Skwir; la sua risposta dice se l’utente lo ha applicato, e come, o lo ha scartato. Uno che agisce diversamente chiede prima all’utente. Le cartelle private non ti vengono mai mostrate.

# draft
control_names_dialog = `{ $command }` indica la finestra che risponde alla finestra Apri o Salva di un altro programma, a cui risponde solo l’utente nella finestra di Skwir

# draft
control_no_script_endpoint = la finestra non ha un endpoint per gli script: riavvia Skwir prima di eseguire questo comando

# draft
control_not_allowed = l’utente non ha permesso l’esecuzione di questo comando

# draft
control_not_listening = Skwir non è aperto, oppure Agenti e script è disattivato: apri Skwir e attivalo in Impostazioni, Plugin

# draft
control_nothing_selected = nulla è selezionato: indica i file in `entries`, con percorso assoluto

# draft
control_nothing_to_paste = niente da incollare: indica i file in `clipboard.entries`, con percorso assoluto, e se sono stati tagliati

# draft
control_plan_waits = il piano attende la revisione dell’utente in Skwir

# draft
control_plugin_description = Consenti ad agenti IA (tramite MCP: skwir mcp) e script (skwir cmd) di pilotare questa finestra. Ciò che modificherebbero viene proposto alla tua revisione. Qualsiasi programma eseguito come te può poi usarlo.

# draft
control_plugin_title = Agenti e script

# draft
control_private = questo indica una cartella privata, che Skwir tiene lontana dagli agenti (Impostazioni, Privacy)

# draft
control_proposer = Un agente

# draft
control_reads_elsewhere = cercare un percorso che questa chiamata legge farebbe raggiungere al sistema un’altra macchina, cosa che un agente non può fare senza permesso: l’utente lo apre prima in Skwir

# draft
control_reads_private = un percorso che questa chiamata legge è privato: Skwir lo tiene lontano dagli agenti (Impostazioni, Privacy)

# draft
control_status = Agenti e script

# draft
control_status_agent = un server MCP avviato con: skwir mcp

# draft
control_status_description = Se questa finestra è in ascolto di agenti e script, dove, e come un agente la raggiunge

# draft
control_status_script = skwir cmd [--apply] <comando> [<argomenti, un oggetto JSON>]

# draft
control_tool_asks = { $description } (prima chiede all’utente nella finestra di Skwir)

# draft
control_unanswered = l’utente non ha risposto: chiedi di nuovo quando è presente

# draft
control_unknown_command = comando sconosciuto `{ $command }`

# draft
control_user_only = `{ $command }` risponde all’utente nella finestra di Skwir: solo l’utente lo esegue
