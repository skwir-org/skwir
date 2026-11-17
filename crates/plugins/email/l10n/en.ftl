# Made from the code by `just words` (tools/words/extract.py): never edit by hand.

# Why an attachment is not previewed. size: the largest the preview reads (16 MB).
email_attachment_too_large = Attachment exceeds { $size } preview limit

# A label in the preview's Properties: how many files the email carries.
email_detail_attachments = Attachments

# A label in the preview's Properties: who the email was copied to (carbon copy).
email_detail_cc = Cc

# A label in the preview's Properties: who sent the email.
email_detail_from = From

# A label in the preview's Properties: when the email was sent.
email_detail_sent = Sent

# A label in the preview's Properties: the email's subject.
email_detail_subject = Subject

# A label in the preview's Properties: who the email was sent to.
email_detail_to = To

# The lines under an email's subject, each on its own line. from, to: the addresses as the message writes them. sent: when it was sent, as the message dates it.
email_header = From: { $from }
    To: { $to }
    Date: { $sent }

# An error: the file could not be read as an email.
email_invalid = Invalid email

# An error: the message no longer holds the attachment asked for.
email_missing_attachment = Missing attachment

# The heading of an email that has no subject.
email_no_subject = No subject

email_plugin_description = Read EML messages: headers, body and attachments, with remote content blocked

email_plugin_title = Email

# A note under an email: pictures it names on the web are not fetched.
email_remote_images = Remote images are not loaded

# A note under an email that carries files: choosing one in the list shows it.
email_select_attachment = Select an attachment to preview it

# The title of an email's preview.
email_title = Email

# Why an email is not previewed. size: the largest the preview reads (16 MB).
email_too_large = Email exceeds { $size } preview limit

# An attachment the email gives no name, in its list of attachments.
email_unnamed_attachment = Attachment
