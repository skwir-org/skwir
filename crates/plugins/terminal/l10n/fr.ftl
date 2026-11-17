# French catalog of skwir-plugin-terminal, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
terminal_follow = Suivre le dossier affiché

# draft
terminal_follow_description = Le terminal se place dans le dossier que montre le panneau, et le panneau va là où vous faites cd.

# draft
terminal_follow_description_more = Rien n’est tapé pendant qu’un programme tourne dans le terminal.

# draft
terminal_plugin_description = Un terminal sous les panneaux (F4) qui va là où va le panneau, et emmène le panneau là où vous faites cd

# draft
terminal_plugin_title = Terminal

# draft
terminal_program = Shell

# draft
terminal_program_description = Le programme que lance le terminal.

# draft
terminal_program_description_more = Laissez vide pour votre shell de connexion.

# draft
terminal_scrollback = Historique

# draft
terminal_scrollback_description = Le nombre de lignes vers lesquelles vous pouvez remonter, pour les terminaux ouverts à partir de maintenant, au plus { $most }.

# draft
terminal_scrollback_description_more = Une fois l’historique plein, chaque tranche de { $lines } lignes d’un terminal large prend environ 30 Mo.

# draft
terminal_scrollback_lines = { $n ->
        [one] { $n } ligne
       *[other] { $n } lignes
    }

# draft
terminal_unknown_quoting = Skwir ne sait pas comment { $shell } protège un chemin : il n’en tape aucun ici

# draft
terminal_unknown_quoting_unnamed = Skwir ne sait pas comment ce shell protège un chemin : il n’en tape aucun ici

# draft
terminal_untypable = Impossible de taper { $name } à une invite de commande
