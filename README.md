# Fetch-ServiceNow-Tickets

A utility that fetches NERSC's ServiceNow tickets and writes them to file in a markdown-liske format for use with AI coding assistants.

## Usage

We have a partial import of the tickets at `/global/cfs/cdirs/nstaff/dingpf/servicenow_incidents` (manually imported whlie we work on bulk downloads).
Running the following will produce a local `/tickets` folder ready for your agent to be pointed at:

```sh
module load python
python3 build_tickets.py
```

## TODO

* import tickets directly from servicenow
* add scron script to refresh tickets regularly
