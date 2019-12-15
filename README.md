# multip

Tiny multi process manager for containers written in Rust. For example if you
want to run nginx and php-fpm in a single container.

> Note: This is not a init replacement. If you need zombie process reaping
> pass `--init` to Docker and it will inject [tini][] for that.

[tini]: https://github.com/krallin/tini

This is somewhat similar to [concurrently], [s6][], [GNU Parallel][parallel]
or some of it's [alternatives][] but intentionally much less featured.

[concurrently]: https://www.npmjs.com/package/concurrently
[s6]: http://skarnet.org/software/s6/
[parallel]: https://www.gnu.org/software/parallel/
[alternatives]: https://www.gnu.org/software/parallel/parallel_alternatives.html

## Features

-   If one the started processes dies it will bring all others down too so your
    container orchestration can handle the error (report, restart, whatever)
-   Prefix process stdout&stderr with labels so you can know which process sent
    which message
-   Signal forwarding to child processes
-   Second SIGINT (ctrl-c) sends SIGTERM instead to the children and third
    sends SIGKILL.

## Installation

Grab a pre-build binary from the releases page

<https://github.com/esamattis/multip/releases>

It depends on libc so no Alpine support at this time.

## Usage

    multip "web: nginx" "php: php-fpm"

The `web:` and `php:` are the prefixes for each processes output. The rest is
passed to `/bin/sh` with `exec`. Ex. `/bin/sh -c "exec nginx"`.

## Advanced features

There are none but you can delegate to wrapper scripts.

### Setting enviroment variables

Create `start.sh` with

```sh
#/bin/sh

set -eu

export API_ENDPOINT=http://api.example/graphql
exec node /app/server.js
```

and call `multip "server: /app/start.sh" "other: /path/to/some/executable"`.

Remember call the actual command with `exec` so it will replace the wrapper
script process instead of starting new subprocess.

### Dropping privileges

If you start `multip` as root you can drop the root privileges with `setpriv` for example

```sh
#!/bin/sh

set -eu

exec setpriv \
    --ruid www-data \
    --rgid www-data \
    --clear-groups \
    node /app/server.js
```

### Automatic restart

```sh
#!/bin/sh

set -eu

while true; do
    ret=0
    node /app/server.js || ret=$?

    echo "Server died with $ret. Restarting soon..."
    sleep 1
done
```

No that here we cannot use `exec` because we need to keep the script alive
for restarts.
