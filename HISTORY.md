# History

ush was created around 2017 at Facebook to handle infrastructure incidents by running parallel ssh across millions of hosts, directly or through jump hosts. The name stands for ultrashell, from the team maintaining hypershell, the service that managed large scale ssh at the time. The joke was that a less intense, dependency-free version of hypershell was needed, hence a static, single binary.

The freq command was added for feature parity with hypershell. Output from exec could be saved and processed later. The file serving functionality of exec -f and the server code were rarely used.

ush was used only briefly before most hypershell production issues were resolved.
