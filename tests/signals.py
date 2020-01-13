#!/usr/bin/env python3

import sys, time, signal

def log(msg):
    print(msg)
    sys.stdout.flush()


def handle_signal(sig, frame):
    log("got signal {}".format(sig))

signal.signal(signal.SIGINT, handle_signal)
signal.signal(signal.SIGTERM, handle_signal)

log("starting")
time.sleep(2)
sys.exit(55)