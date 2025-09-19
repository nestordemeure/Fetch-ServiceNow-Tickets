# Fetch-ServiceNow-Tickets

A utility that fetches NERSC's ServiceNow tickets and writes them to file in a markdown-liske format for use with AI coding assistants.

## Install

will require python and request

## Usage

**TODO**

## Notes

API explorer: <https://nersc.servicenowservices.com/nav_to.do?uri=%2F$restapi.do>

api key?
-> AI bot key access

### Table API

sys_id are invident id's in the database

syspam are fields we want
can specify the fields we want, a lot of those are dead
syspam_display_value=true to get fields

table api not performant

### Scripted API

for large queries or frequent ones

g_ner namespace

give it a sys_id (/ number), get nice redeable format

### API Endpoint

we want:

* bulk querying of all tickets starting a number of months ago (1, 12, all ever)
* including header (possibility to restrict fields for efficiency)
* including attachements

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

## TODO

* download raw tickets
* export them to a human redeable format
  * define format
    * marp style header
    * normal markdown for ticket contents
    * a way to seperate between tickets and indicate the type of the next ticket (user, date?, internal/user-visible)
  * deal with attachements?
    * in an `attachements/<ticket-number>/<files>` folder?
* create an AGENTS.md` file documentating the folder structure and format
* add scron script to refresh tickets
