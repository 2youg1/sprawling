#!/bin/sh
# Raises a city in this folder if there is not one yet, serves it, and
# opens the WebUI. This terminal is the city: Ctrl-C stops the city.
cd "$(dirname "$0")" || exit 1
exec ./sprawling up
