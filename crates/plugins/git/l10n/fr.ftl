# French catalog of skwir-plugin-git, drafted by a model from en.ftl (ADR-139, step G): every message is a draft until reviewed.

# draft
git_column_author = Auteur

# draft
git_column_commit = Commit

# draft
git_column_date = Date

# draft
git_column_hash = Empreinte

# draft
git_column_names = Branches et étiquettes

# draft
git_commits = { $count }

# draft
git_commits_shown = { $count ->
        [one] { $count } commit affiché
       *[other] { $count } commits affichés
    }

# draft
git_detail_branch = Branche

# draft
git_detail_commits = Commits

# draft
git_history = Historique Git

# draft
git_history_on = Historique Git · { $branch }

# draft
git_names_cut = Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées

# draft
git_newest = { $count ->
        [1] Seul le commit le plus récent
        [one] Le commit le plus récent
       *[other] Les { $count } commits les plus récents
    }

# draft
git_newest_names_cut = { $count ->
        [1] Seul le commit le plus récent. Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées
        [one] Le commit le plus récent. Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées
       *[other] Les { $count } commits les plus récents. Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées
    }

# draft
git_no_commit = Aucun commit pour l’instant

# draft
git_no_commit_changed = Aucun commit ne l’a modifié

# draft
git_not_in_repository = Pas dans un dépôt Git

# draft
git_plugin_description = L’historique des commits d’un dossier et d’un fichier dans un dépôt Git, le graphe de ses fusions et ce que chaque commit a modifié, ainsi que l’état des fichiers marqué dans les vues

# draft
git_plugin_title = Git

# draft
git_requirement = Historiques des commits

# draft
git_states_setting = Afficher l’état Git des fichiers

# draft
git_states_setting_description = Marquer les fichiers modifiés, indexés, nouveaux ou en conflit, et les dossiers qui les contiennent, dans les dossiers d’un dépôt Git.

# draft
git_states_setting_description_more = La colonne Git de la liste indique la même chose.

# draft
git_stopped = { $count ->
        [1] Seul le commit le plus récent : lecture arrêtée avant le plus ancien
        [one] Le commit le plus récent : lecture arrêtée avant le plus ancien
       *[other] Les { $count } commits les plus récents : lecture arrêtée avant le plus ancien
    }

# draft
git_stopped_names_cut = { $count ->
        [1] Seul le commit le plus récent : lecture arrêtée avant le plus ancien. Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées
        [one] Le commit le plus récent : lecture arrêtée avant le plus ancien. Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées
       *[other] Les { $count } commits les plus récents : lecture arrêtée avant le plus ancien. Certains commits ont plus de branches et d’étiquettes que celles qui sont affichées
    }

# draft
git_way = Historique

# draft
git_way_tip = Son historique des commits, lu dans Git
