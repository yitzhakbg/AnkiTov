#!/bin/bash
export DISPLAY=:20
export PYTHONPATH=/stream:/opt/anki/app_packages:/opt/anki/python/lib/python3.13
export PYTHONUNBUFFERED=1
export PYTHONFAULTHANDLER=1
exec /opt/anki/python/bin/python3 -u /stream/boot.py -b /home/ubuntu/.local/share/Anki2
