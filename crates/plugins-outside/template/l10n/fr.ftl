# French catalog of the template (word-count), drafted by a model from en.ftl (ADR-139, step H): every message is a draft until reviewed.

# draft
plugin-title = Nombre de mots

# draft
plugin-description = Le nombre de mots d’un texte, dans les Propriétés et pour la sélection

# draft
words-label = Mots

# draft
count-title = Compter les mots

# draft
count-description = Le nombre de mots des fichiers, de chacun et de tous : la sélection, ou les éléments donnés

# draft
copy-title = Copier le nombre de mots

# draft
copy-description = Le nombre de mots de tous les fichiers, en un nombre pour le presse-papiers : la sélection, ou les éléments donnés

# draft
count-said = { $count ->
        [one] { $count } mot en tout
       *[other] { $count } mots en tout
    }

# draft
no-command = aucune commande { $command }
