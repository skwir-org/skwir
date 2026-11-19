# French catalog of skwir-plugin-control, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
control_another_listens = une autre fenêtre de Skwir est à l’écoute

# draft
control_ask = Un agent demande à exécuter { $command }

# draft
control_ask_detail = { $command } : { $description }

# draft
control_ask_detail_arguments = { $command } : { $description }
    { $arguments }

# draft
control_cannot_listen = La fenêtre ne peut pas écouter les agents et les scripts : { $error }

# draft
control_channel_folder = le dossier du canal

# draft
control_closed_unasked = Skwir s’est fermé avant que l’utilisateur réponde

# draft
control_closed_unreviewed = Skwir s’est fermé avant que le plan reçoive une réponse

# draft
control_closing = Skwir se ferme

# draft
control_elsewhere = consulter { $path } amènerait le système à joindre une autre machine, ce qu’un agent ne peut pas faire sans qu’on le lui demande : l’utilisateur doit d’abord l’ouvrir dans Skwir

# draft
control_instructions = Skwir, un gestionnaire de fichiers. Chaque outil est l’une de ses commandes, ses points écrits sous la forme de deux tirets bas. Nommez les fichiers sur lesquels agit une commande dans son argument `entries`, par chemin absolu : la sélection est celle de l’utilisateur et peut changer entre deux appels (l’outil `entries` liste ce que montre un panneau, ou avec `location` un dossier, sans changer ce que voit l’utilisateur). Une commande qui modifierait des fichiers propose un plan que l’utilisateur examine dans la fenêtre de Skwir ; sa réponse dit si l’utilisateur l’a appliqué, et comment, ou s’il l’a écarté. Une commande qui agit autrement demande d’abord à l’utilisateur. Les dossiers privés ne vous sont jamais montrés.

# draft
control_names_dialog = `{ $command }` désigne la fenêtre qui répond à la boîte de dialogue Ouvrir ou Enregistrer d’un autre programme, à laquelle seul l’utilisateur répond dans la fenêtre de Skwir

# draft
control_no_script_endpoint = la fenêtre n’a pas de point d’accès pour les scripts : redémarrez Skwir avant d’exécuter cette commande

# draft
control_not_allowed = l’utilisateur n’a pas autorisé l’exécution de cette commande

# draft
control_not_listening = Skwir n’est pas ouvert, ou Agents et scripts est désactivé : ouvrez Skwir et activez-le dans Paramètres, Modules

# draft
control_nothing_selected = rien n’est sélectionné : nommez les fichiers dans `entries`, par chemin absolu

# draft
control_nothing_to_paste = rien à coller : nommez les fichiers dans `clipboard.entries`, par chemin absolu, et dites s’ils ont été coupés

# draft
control_plan_waits = le plan attend l’examen de l’utilisateur dans Skwir

# draft
control_plugin_description = Laisser les agents d’IA (par MCP : skwir mcp) et les scripts (skwir cmd) piloter cette fenêtre. Ce qu’ils modifieraient vous est proposé pour examen. Tout programme lancé sous votre compte peut alors s’en servir.

# draft
control_plugin_title = Agents et scripts

# draft
control_private = ceci nomme un dossier privé, que Skwir tient à l’écart des agents (Paramètres, Confidentialité)

# draft
control_proposer = Un agent

# draft
control_status = Agents et scripts

# draft
control_status_agent = un serveur MCP lancé par : skwir mcp

# draft
control_status_description = Indique si cette fenêtre écoute les agents et les scripts, où, et comment un agent la joint

# draft
control_status_script = skwir cmd [--apply] <commande> [<arguments, un objet JSON>]

# draft
control_tool_asks = { $description } (demande d’abord à l’utilisateur dans la fenêtre de Skwir)

# draft
control_unanswered = l’utilisateur n’a pas répondu : redemandez quand il sera là

# draft
control_unknown_command = commande inconnue `{ $command }`

# draft
control_user_only = `{ $command }` répond à l’utilisateur dans la fenêtre de Skwir : seul l’utilisateur peut l’exécuter
