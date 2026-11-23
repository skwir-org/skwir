# Italian catalog of skwir-plugin-shrink, drafted by a model from en.ftl (ADR-139, docs/glossary-it.md): every message is a draft until reviewed.

# draft
shrink_already = { $copy } era già una copia ridotta di { $file }

# draft
shrink_already_there = { $name } è già presente

# draft
shrink_as_small = { $name } è già il più piccolo possibile: non è stata creata alcuna copia

# draft
shrink_at_most = Al massimo

# draft
shrink_at_most_placeholder = Nessun limite, oppure una dimensione: 2 MB, 500 KB

# draft
shrink_at_most_refused = Al massimo: { $why }

# draft
shrink_both = { $pixels ->
        [one] Immagini a un massimo di { $dpi } dpi; immagini di { $pixels } pixel
       *[other] Immagini a un massimo di { $dpi } dpi; immagini di { $pixels } pixel
    }

# draft
shrink_cancelled = La creazione della copia è stata annullata

# draft
shrink_clash_cancelled = Annullato: non è stato ridotto nulla

# draft
shrink_closing = Skwir si sta chiudendo

# draft
shrink_dialog_accept = { $count ->
        [1] Crea copia
       *[other] Crea copie
    }

# draft
shrink_dialog_intro = { $count ->
        [1] Una copia ridotta viene creata accanto al file, che resta invariato.
       *[other] Una copia ridotta viene creata accanto a ogni file, che restano invariati.
    }

# draft
shrink_dialog_title = { $count ->
        [1] Riduci dimensione di { $name }
       *[other] Riduci dimensione di { $count } file
    }

# draft
shrink_gray = In grigio

# draft
shrink_gray_help = In scala di grigi

# draft
shrink_gray_help_pdfs = Immagini, e immagini di un PDF, in scala di grigi

# draft
shrink_in_place_only = { $name } accetta solo copie scritte sul posto: lì non viene creata alcuna copia ridotta

# draft
shrink_lossless = Senza perdita

# draft
shrink_lossless_at_most = La qualità `lossless` non ricodifica nulla, quindi non può mantenere una copia sotto una certa dimensione: indica `at_most` con la qualità `print`, `screen` o `small`

# draft
shrink_lossless_jpeg = «Senza perdita» lascia un JPEG invariato: scegli Stampa, Schermo o Piccola, oppure rimuovi i dettagli di posizione e fotocamera

# draft
shrink_made = creata { $copy }, una copia ridotta di { $file }

# draft
shrink_make = creare { $copy }, una copia ridotta di { $file }

# draft
shrink_make_verb = Crea copia ridotta

# draft
shrink_no_folder = Il file non si trova in alcuna cartella

# draft
shrink_no_size = { $typed } non è una dimensione

# draft
shrink_no_unit = { $unit } non è un’unità: scrivi KB o MB

# draft
shrink_none_made_note = Non è stata creata alcuna copia ridotta di { $name }: { $said }

# draft
shrink_none_made_strip = Non è stata creata alcuna copia ridotta di { $name }, quindi nemmeno i suoi dettagli di posizione e fotocamera sono stati rimossi

# draft
shrink_not_a_folder = { $name } non è una cartella

# draft
shrink_not_a_picture = { $name } non è né un’immagine JPEG o PNG né un PDF

# draft
shrink_not_placed = Non è stato possibile mettere { $name } in { $folder }

# draft
shrink_not_under = Non è stato possibile portare { $name } sotto la dimensione richiesta: la sua copia più piccola è di { $size }

# draft
shrink_note = { $name }: { $said }

# draft
shrink_nothing = Niente da ridurre

# draft
shrink_nothing_made = Non verrà ridotto nulla finché non scegli.

# draft
shrink_nothing_re_encoded = Nessuna ricodifica

# draft
shrink_pdf_dpi = Immagini a un massimo di { $dpi } dpi

# draft
shrink_picture_pixels = { $pixels ->
        [one] Al massimo { $pixels } pixel di larghezza
       *[other] Al massimo { $pixels } pixel di larghezza
    }

# draft
shrink_plan = { $count ->
        [1] creare una copia ridotta di { $name }
       *[other] creare copie ridotte di { $count } file
    }

# draft
shrink_plugin_description = Crea copie ridotte di immagini e PDF accanto a essi: Riduci dimensione…, in Comprimi e nel menu di un’anteprima

# draft
shrink_plugin_off = Le copie ridotte richiedono il plugin attivo

# draft
shrink_plugin_title = Copie ridotte

# draft
shrink_print = Stampa

# draft
shrink_quality = Qualità

# draft
shrink_reduce = Crea copie ridotte

# draft
shrink_reduce_ask_description = Chiede quanto ridurre, poi crea una copia ridotta di ogni immagine e PDF accanto a esso, come fa `shrink.reduce`

# draft
shrink_reduce_description = Crea una copia ridotta di ogni immagine (JPEG, PNG) e PDF accanto a esso, o in `dest`, chiamata `name (reduced).ext`: `quality` lossless, print (immagini di 3840 pixel di larghezza, immagini di un PDF a 300 dpi), screen (1920, 150 dpi) o small (1280, 96 dpi); `gray`; `strip`, che esclude i dettagli di posizione e fotocamera di un’immagine; `at_most`, una dimensione entro cui ogni copia deve rientrare, provando passaggi via via più piccoli. Una copia che non sarebbe più piccola non viene creata (piano)

# draft
shrink_reduce_size = Riduci dimensione…

# draft
shrink_requirement = Creare copie ridotte

# draft
shrink_same_pixels = Stessi pixel, nulla va perso

# draft
shrink_screen = Schermo

# draft
shrink_small = Piccola

# draft
shrink_strip = Rimuovi dettagli di posizione e fotocamera

# draft
shrink_strip_help = La data di scatto resta

# draft
shrink_went_missing = { $name } è scomparso durante la creazione

# draft
shrink_work_folder = Una cartella in cui creare la copia
