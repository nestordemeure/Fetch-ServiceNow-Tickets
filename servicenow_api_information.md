# ServiceNow API access

API explorer: <https://nersc.servicenowservices.com/nav_to.do?uri=%2F$restapi.do>

## Table API

sys_id are invident id's in the database

syspam are fields we want
can specify the fields we want, a lot of those are dead
syspam_display_value=true to get fields

table api not performant

## Scripted API

for large queries or frequent ones

g_ner namespace

give it a sys_id (/ number), get nice redeable format

## API Endpoint

we want:

* bulk querying of all tickets starting a number of months ago (1, 12, all ever)
* including header (possibility to restrict fields for efficiency)
* including attachements

## Daniel's Basic doc:

You should be able to use this service user to gather comments by `sys_id` in one of two ways:

Table API:

```sh
curl "https://nersctest.servicenowservices.com/api/now/table/incident/04347c741be43e90ac81a820f54bcb3d?sysparm_display_value=true&sysparm_exclude_reference_link=true&sysparm_fields=number%2C%20comments%2C%20work_notes" \
--request GET \
--header "Accept:application/json" \
--user "username":"password"

{
   "result": {
      "number": "INC0243086",
      "comments": "",
      "work_notes": "2025-10-10 16:23:54 - Daniel Gens (dygens) (Staff work notes (NERSC private))\nTest work note\n\n"
   }
}
```

Incident Utils:

```sh
curl "https://nersctest.servicenowservices.com/api/g_ner/incident_utils/get_journal_entries?sys_id=04347c741be43e90ac81a820f54bcb3d" \
--request GET \
--header "Accept:application/json" \
--user "username":"password"

{
   "result": {
      "sys_id": "04347c741be43e90ac81a820f54bcb3d",
      "number": "INC0243086",
      "comments": [],
      "work_notes": [
         {
            "created_by": "dygens",
            "created_on": "2025-10-10 23:23:54",
            "sys_id": "bb14a58d1b603210e1920f67624bcb0d",
            "text": "Test work note"
         }
      ]
   }
}
```