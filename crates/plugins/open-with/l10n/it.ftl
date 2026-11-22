# Italian catalog of skwir-plugin-open-with, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
open_with_already_opens = { $app } apre già { $kind ->
        [folder] le cartelle
       *[file] i file di tipo { $description }
    }

# draft
open_with_always_use = Usa sempre per { $kind ->
        [folder] le cartelle
       *[file] i file di tipo { $description }
    }

# draft
open_with_cancelled = apertura annullata

# draft
open_with_change = Cambia…

# draft
open_with_choices = Elenca le applicazioni per la selezione

# draft
open_with_choices_description = Elenca le applicazioni che aprono le voci selezionate (o la cartella del riquadro con `here`), prima quelle consigliate, ciascuna con l’invocazione `open_with` che le apre; con `defaults`, ciascuna con il `open_with.set_default` che la rende predefinita per il loro tipo

# draft
open_with_chooser = Apri con

# draft
open_with_chooser_title = { $count ->
        [1] Apri { $name } con
       *[other] Apri { $count } elementi con
    }

# draft
open_with_default = Predefinita

# draft
open_with_default_generic = Predefinita · { $generic }

# draft
open_with_does_not_exist = { $path } non esiste

# draft
open_with_edit_associations = Riscrivi le righe dell’applicazione predefinita di un tipo

# draft
open_with_edit_associations_description = Imposta le righe che un tipo MIME ha in un mimeapps.list (associazioni predefinite, aggiunte e rimosse); ciò che eseguono Annulla e Ripeti per un’applicazione predefinita

# draft
open_with_file_manager = Gestore di file predefinito

# draft
open_with_file_manager_another = Un altro programma

# draft
open_with_file_manager_description = Quale applicazione apre le cartelle per ogni programma: la stessa scelta di Usa sempre di Apri con su una cartella, e delle impostazioni del desktop.

# draft
open_with_file_manager_description_more = Con Skwir scelto, Mostra nella cartella di un altro programma apre la cartella in Skwir con i suoi file selezionati, anche quando Skwir non è in esecuzione.

# draft
open_with_file_manager_held = { $program } mostra ancora i file che gli altri programmi mostrano nella loro cartella, finché non termina

# draft
open_with_file_manager_list = Il gestore di file predefinito

# draft
open_with_file_manager_list_description = L’applicazione che apre le cartelle per ogni programma, come la elencano le Impostazioni: le applicazioni che aprono le cartelle tra cui scegliere, ciascuna con il `open_with.set_default` che la rende predefinita, e se Skwir mostra i file che gli altri programmi mostrano nella loro cartella. Risponde dalla memoria

# draft
open_with_file_manager_not_installed = Skwir non può essere scelto qui: la sua voce del desktop non è installata

# draft
open_with_file_manager_offer = Se scelto, Skwir mostra anche i file che gli altri programmi mostrano nella loro cartella

# draft
open_with_file_manager_reading = Lettura delle applicazioni che aprono le cartelle…

# draft
open_with_file_manager_shows = Skwir mostra anche i file che gli altri programmi mostrano nella loro cartella

# draft
open_with_file_manager_unchosen = Non scelto

# draft
open_with_file_manager_unchosen_opens = Non scelto ({ $app })

# draft
open_with_menu = Indica l’applicazione predefinita in un menu

# draft
open_with_menu_description = L’Apri di un menu per la selezione, o per i file indicati (quelli dell’anteprima: `entries`, come li elenca il riquadro), con il nome dell’applicazione predefinita che condividono, e le altre applicazioni per i loro tipi nel sottomenu Apri con; nessuna voce quando i loro tipi o le applicazioni non sono ancora noti. Risponde dalla memoria

# draft
open_with_no_application = Nessuna applicazione

# draft
open_with_no_applications = Nessuna applicazione trovata

# draft
open_with_no_location = il riquadro non ha una posizione

# draft
open_with_no_such_pane = nessun riquadro di questo tipo

# draft
open_with_none_opens = Nessuna applicazione apre { $kind ->
        [folder] le cartelle
       *[file] i file di tipo { $description }
    }

# draft
open_with_not_a_file = { $name } non è un file: Skwir non lo apre

# draft
open_with_not_changed = L’applicazione predefinita non è stata cambiata: { $error }

# draft
open_with_not_installed = nessuna applicazione { $id } è installata

# draft
open_with_not_your_list = { $path } non è uno dei tuoi elenchi di applicazioni predefinite

# draft
open_with_nothing_to_open = niente da aprire

# draft
open_with_now = Ora

# draft
open_with_now_opens = { $app } ora apre { $kind ->
        [folder] le cartelle
       *[file] i file di tipo { $description }
    }

# draft
open_with_one_type_at_a_time = Un’applicazione predefinita si sceglie per un tipo di file alla volta

# draft
open_with_open = Apri

# draft
open_with_open_description = Entra nella cartella selezionata, o apre i file selezionati con l’applicazione predefinita del loro tipo, come la scelgono gli elenchi del desktop (quella che nomina il menu contestuale)

# draft
open_with_open_files_with = Apri { $kind ->
        [folder] le cartelle
       *[file] i file di tipo { $description }
    } con

# draft
open_with_open_terminal = Apri terminale qui

# draft
open_with_open_terminal_description = Apre un terminale nella cartella selezionata, altrimenti nella cartella mostrata, altrimenti (in una ricerca) nella cartella che contiene la selezione

# draft
open_with_open_terminal_menu = Apri terminale qui

# draft
open_with_open_with = Apri con un’applicazione tramite ID desktop

# draft
open_with_open_with_app = Apri con { $app }

# draft
open_with_open_with_description = Apre le voci selezionate (o la cartella del riquadro con `here`) con l’applicazione il cui ID del file desktop è `app`

# draft
open_with_opens_with = Si apre con

# draft
open_with_other_application = Altra applicazione…

# draft
open_with_other_applications = Altre applicazioni

# draft
open_with_plugin_description = Apri i file con la loro applicazione predefinita o una a tua scelta, e apri un terminale qui

# draft
open_with_plugin_title = Apri con

# draft
open_with_properties = Indica l’applicazione predefinita nelle Proprietà

# draft
open_with_properties_description = La riga Si apre con delle Proprietà per un file: l’applicazione con cui si apre il suo tipo e il modo di cambiarla

# draft
open_with_recommended = Consigliate

# draft
open_with_related = Correlate

# draft
open_with_search_applications = Cerca applicazioni

# draft
open_with_set_default = Imposta l’applicazione predefinita per un tipo

# draft
open_with_set_default_description = Rende l’applicazione il cui ID del file desktop è `app` quella predefinita per il tipo MIME `mime`, nel mimeapps.list dell’utente; la notifica offre Annulla
