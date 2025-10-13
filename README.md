# Fetch-ServiceNow-Tickets

A utility that fetches NERSC's ServiceNow tickets and writes them to file in a markdown-liske format for use with AI coding assistants.

## Install

will require python and request

## Usage

**TODO**

### Tickets Representation

Folder organization:

```sh
/tickets/
  /<ticket-number.md> # individual tickets as one markdown file each
  /attachements/ # folder for attachements
    /<ticket-number>/ # folder per ticket with attachements
      /<filename> # actual attachement
```

Question: is the filesystem file with that many small files? do we want to split them by month to ease things up and let the model "intuitively" know how old a ticket is?

Representation:

```md
ticket name: SOMETHING
status: RESOLVED
date: SOMETHING
---
<--user:name-->

markdown text here

<--user:name type:internal-message-->
markdown here
```

Question: what do we realy need in the header? what do we want for seperators? How do we deal gracefully with attachements?

Or maybe a more base-markdown approach:
level one heading for title
a couple lines for metadata
level two headings for title of subsequent messages (titled after the username, data, and type of essage in parenthesis if need be)

## TODO

* download raw tickets
* export them to a human redeable format
  * deal with attachements?
    * in an `attachements/<ticket-number>/<files>` folder?
* create an AGENTS.md` file documentating the folder structure and format
* add scron script to refresh tickets
