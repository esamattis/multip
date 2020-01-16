# multip

Tiny multi process `init` for containers written in Rust. For example if you
want to run nginx and php-fpm in a single container.

This is very similiar to [concurrently][] but also acts as a valid `init` by
implementing zombie process reaping and signal forwarding. You could think
this as a combination of `tini` (the default `init` in Docker) and
`concurrently`.

> Wait. Containers should run only a single process, right? [Read this](#multiprocess-containers)

[concurrently]: https://www.npmjs.com/package/concurrently

## Features

-   If one the started processes exits it will bring all others down too so
    your container orchestration can handle the error (report, restart, whatever)
-   Reap zombies
-   Prefix process stdout&stderr with labels so you can know which process sent
    which message
-   Signal forwarding to child processes
-   Second SIGINT (ctrl-c) sends SIGTERM instead to the children and third
    sends SIGKILL.
-   The exit code of `multip` will be the one used by the first dead child

## Installation

Grab a pre-build binary from the releases [page][].

[page]: https://github.com/esamattis/multip/releases

The binary is statically linked with musl libc so it will run in bare bones
distros too such as Alpine Linux.

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

Note that here we cannot use `exec` because we need to keep the script alive
for restarts.

### Keep running on success

`multip` brings all processes down even when child exits with success status
code (zero). You can keep others running with `sleep infinity`.

```sh
#!/bin/sh

set -eu

ret=0
node /app/server.js || ret=$?

if [ "$ret" = "0" ]; then
    exec sleep infinity
fi

exit $ret
```

# Similar tools

Inits

-   tini https://github.com/krallin/tini
    -   The default `init` shipped with Docker
-   dump-init https://github.com/Yelp/dumb-init
-   catatonit https://github.com/openSUSE/catatonit
-   s6 http://skarnet.org/software/s6/
    -   More complete init system but still fairly small

Plain multiprocess runners

-   concurrently https://www.npmjs.com/package/concurrently
-   GNU Parallel https://www.gnu.org/software/parallel/
    -   Alternatives https://www.gnu.org/software/parallel/parallel_alternatives.html

# Multiprocess containers?

In reality most your containers are multiprocess containers any way if they
happen to use worker processes or spawn out processes to do one off tasks. So
there' nothing technically wrong with them.

It is usually a good design to create single purpose containers but it's not
always the best approach. For example if you want to serve PHP apps with
php-fpm it is difficult to do with single process containers because php-fpm
does not speak HTTP but FastCGI so you need some web server to translate
FastCGI to HTTP. You can run nginx in a separate container which proxies to
the php-fpm container but because php-fpm cannot share static files you must
deploy your code to both containers which can be a hassle to manage.

With `multip` it is possible to create a container which runs multiple
processes but acts like it has only one process with minimal overhead.
