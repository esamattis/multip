const { execSync } = require("child_process");
const pid = process.pid;

console.log(`start ${pid}`);

process.on("SIGINT", () => {
    console.log("Received SIGINT.");
    // process.exit(2);
});

process.on("SIGTERM", () => {
    console.log("Received SIGTERM.");
    process.exit(4);
});

process.on("SIGUSR1", () => {
    console.log("Received SIGUSR1.");
    // process.exit(4);
});

setTimeout(() => {
    process.stdout.write(execSync("head -c 100 /dev/urandom"));
}, 1000);

function rand() {
    return Math.round(Math.random() * 1000);
}

function log() {
    const msg = process.argv[2];
    process.stdout.write(`${pid} ${msg}\n`);
    setTimeout(log, 1000 + rand());
}

function err() {
    process.stderr.write(`${pid} error message\n`);
    setTimeout(err, (1000 + rand()) * 5);
}

// setTimeout(() => {
//     process.exit(44);

// }, rand())

log();
err();
