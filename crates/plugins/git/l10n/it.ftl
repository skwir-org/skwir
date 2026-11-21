# Italian catalog of skwir-plugin-git, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
git_column_author = Autore

# draft
git_column_commit = Commit

# draft
git_column_date = Data

# draft
git_column_hash = Hash

# draft
git_column_names = Rami ed etichette

# draft
git_commits = { $count }

# draft
git_commits_shown = { $count ->
        [one] { $count } commit mostrato
       *[other] { $count } commit mostrati
    }

# draft
git_detail_branch = Ramo

# draft
git_detail_commits = Commit

# draft
git_history = Cronologia Git

# draft
git_history_on = Cronologia Git · { $branch }

# draft
git_names_cut = Alcuni commit hanno più rami ed etichette di quelli mostrati

# draft
git_newest = { $count ->
        [1] Solo il commit più recente
       *[other] Gli ultimi { $count } commit
    }

# draft
git_newest_names_cut = { $count ->
        [1] Solo il commit più recente. Alcuni commit hanno più rami ed etichette di quelli mostrati
       *[other] Gli ultimi { $count } commit. Alcuni commit hanno più rami ed etichette di quelli mostrati
    }

# draft
git_no_commit = Ancora nessun commit

# draft
git_no_commit_changed = Nessun commit l’ha modificato

# draft
git_not_in_repository = Non si trova in un repository Git

# draft
git_plugin_description = La cronologia dei commit di una cartella e di un file in un repository Git, il grafico delle sue unioni e ciò che ogni commit ha modificato, e lo stato dei file indicato nelle viste

# draft
git_plugin_title = Git

# draft
git_requirement = Cronologie dei commit

# draft
git_states_setting = Mostra lo stato Git dei file

# draft
git_states_setting_description = Contrassegna i file modificati, in staging, nuovi o in conflitto, e le cartelle che li contengono, nelle cartelle conservate in un repository Git.

# draft
git_states_setting_description_more = La colonna Git dell’elenco indica lo stesso.

# draft
git_stopped = { $count ->
        [1] Solo il commit più recente: la lettura si è interrotta prima del più vecchio
       *[other] Gli ultimi { $count } commit: la lettura si è interrotta prima del più vecchio
    }

# draft
git_stopped_names_cut = { $count ->
        [1] Solo il commit più recente: la lettura si è interrotta prima del più vecchio. Alcuni commit hanno più rami ed etichette di quelli mostrati
       *[other] Gli ultimi { $count } commit: la lettura si è interrotta prima del più vecchio. Alcuni commit hanno più rami ed etichette di quelli mostrati
    }

# draft
git_way = Cronologia

# draft
git_way_tip = La sua cronologia dei commit, letta da Git
