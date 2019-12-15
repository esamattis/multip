const pid = process.pid;

console.log(`start ${pid}`);

process.on("SIGINT", () => {
    console.log("Received SIGINT.");
});

process.on("SIGTERM", () => {
    console.log("Received SIGTERM.");
});

function rand() {
    return Math.round(Math.random() * 1000);
}

function log() {
    const msg = process.argv[2];
    // process.stdout.write(`${pid} ${msg}\n`);
    setTimeout(log, 1000 + rand());
}

function err() {
    // process.stderr.write(`${pid} error message\n`);
    setTimeout(err, 10000 + rand() * 5);
}

// setTimeout(() => {
//     process.exit(44);

// }, rand())

log();
err();
