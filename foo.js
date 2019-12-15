// console.log("start")

const pid = process.pid;

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
