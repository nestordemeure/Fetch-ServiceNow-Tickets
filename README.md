# Fetch-ServiceNow-Tickets

A utility that fetches NERSC's ServiceNow tickets and writes them to file in a markdown-liske format for use with AI coding assistants.

## Information

The tickets are currenlty stored at `/global/cfs/cdirs/nstaff/dingpf/servicenow_incidents`.
They have been manualy imported while we wait for proper bulk download.

## TODO

* import tickets directly form servicenow
* export tickets to a human redeable format
* create an AGENTS.md` file documentating the folder structure and format
* add scron script to refresh tickets
