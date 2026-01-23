# Fetch-ServiceNow-Tickets

A utility that fetches NERSC's ServiceNow tickets and writes them to file in a markdown-liske format for use with AI coding assistants.

## Usage

We have a partial import of the tickets at `/global/cfs/cdirs/nstaff/dingpf/servicenow_incidents` (manually imported while we work on bulk downloads).
Running the following will produce a local `/tickets` folder with an `AGENTS.md` file ready for your agents to be pointed at:

```sh
module load python
python3 format_tickets.py
```

See [`ticket_format_specification.md`](./ticket_format_specification.md) for a detailled specification of the ticket's representation.

## TODO

* import tickets directly from servicenow
* add scron script to refresh tickets regularly