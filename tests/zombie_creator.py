#!/usr/bin/env python3

import time, os, sys

def log(msg):
    print(msg)
    sys.stdout.flush()


# Split into a worker
worker_pid = os.fork()

if worker_pid != 0:
    # The parent just waits for the worker to exit
    os.waitpid(worker_pid, 0)
    log("Main: Worker exit captured. Sleeping.")
    # and wait a bit for it to out live orphans
    time.sleep(0.3)
    log("Main exiting...")
    sys.exit(11)


child_pid = os.fork()

if child_pid == 0:
    # Sleep longer than the parent so this becomes a zombie when exiting as
    # it has no parent that can wait on it
    log("Child started")
    time.sleep(0.2)
    log("Orphan exiting and becoming zombie")
    sys.exit(12)
else:
    log("Created child {}".format(child_pid))
    time.sleep(0.1)
    log("Worker exiting, making the child orphan")
    # Exit before the child without waiting for it so it becomes an orphan
    sys.exit(13)
