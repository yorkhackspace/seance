# Seance

Seance is a tool for talking to CNC machines that speak HPGL (e.g. some laser cutters).
The current state of this tool is very much work-in-progress.

## Linux
You will need the `usblp` kernel module loaded.
Add your user to the `lp` group.

## Printer Server

Seance relies on there being a printer server (e.g. CUPS) connected to the laser cutter. This can be set up on a Pi by building and installing the `ouija` package in this repository. This will set up CUPS and create a user called `planchette` that is a member of the `lpadmin` group. After installing the `ouija` package, a password can be set on this user to allow it to access the CUPS admin interface.

The `ouija` package can be built by running `build-ouija-deb.sh`, assuming you have `dpkg` installed.
