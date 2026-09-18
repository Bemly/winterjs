//! `node:internal/zip/fs-util`（Node lib/internal/zip/fs-util.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/fs-util.js.
import errors from 'node:internal/errors';
import fsMod from 'node:fs';
const {
  codes: {
    ERR_INVALID_STATE: { HideStackFramesError: ERR_INVALID_STATE },
    ERR_ZIP_INVALID_ARCHIVE: { HideStackFramesError: ERR_ZIP_INVALID_ARCHIVE },
  },
} = errors;
const fs = fsMod.default ?? fsMod;

function fsOpenAsync(path, flag) {
  return new Promise((resolve, reject) => {
    fs.open(path, flag, (err, fd) => (err ? reject(err) : resolve(fd)));
  });
}

function fsStatAsync(path) {
  return new Promise((resolve, reject) => {
    fs.stat(path, (err, stats) => (err ? reject(err) : resolve(stats)));
  });
}

function fsLstatAsync(path) {
  return new Promise((resolve, reject) => {
    fs.lstat(path, (err, stats) => (err ? reject(err) : resolve(stats)));
  });
}

function fsReadlinkAsync(path) {
  return new Promise((resolve, reject) => {
    fs.readlink(path, 'utf8', (err, target) => (err ? reject(err) : resolve(target)));
  });
}

function fsCloseAsync(fd) {
  return new Promise((resolve, reject) => {
    fs.close(fd, (err) => (err ? reject(err) : resolve()));
  });
}

function fsFstatAsync(fd) {
  return new Promise((resolve, reject) => {
    fs.fstat(fd, (err, stats) => (err ? reject(err) : resolve(stats)));
  });
}

function fsReadAsync(fd, buffer, offset, length, position) {
  return new Promise((resolve, reject) => {
    fs.read(fd, buffer, offset, length, position, (err, bytesRead) => (err ? reject(err) : resolve(bytesRead)));
  });
}

function fsWriteAsync(fd, buffer, offset, length, position) {
  return new Promise((resolve, reject) => {
    fs.write(fd, buffer, offset, length, position, (err, bytesWritten) => (err ? reject(err) : resolve(bytesWritten)));
  });
}

function fsFtruncateAsync(fd, len) {
  return new Promise((resolve, reject) => {
    fs.ftruncate(fd, len, (err) => (err ? reject(err) : resolve()));
  });
}

async function readFdFully(fd, buffer, position) {
  let done = 0;
  while (done < buffer.length) {
    const bytesRead = await fsReadAsync(fd, buffer, done, buffer.length - done, position + done);
    if (bytesRead <= 0) {
      throw new ERR_ZIP_INVALID_ARCHIVE('unexpected end of file');
    }
    done += bytesRead;
  }
}

function readFdFullySync(fd, buffer, position) {
  let done = 0;
  while (done < buffer.length) {
    const bytesRead = fs.readSync(fd, buffer, done, buffer.length - done, position + done);
    if (bytesRead <= 0) {
      throw new ERR_ZIP_INVALID_ARCHIVE('unexpected end of file');
    }
    done += bytesRead;
  }
}

async function writeFdFully(fd, buffer, position) {
  let done = 0;
  while (done < buffer.length) {
    const bytesWritten =
      await fsWriteAsync(fd, buffer, done, buffer.length - done, position + done);
    if (bytesWritten <= 0) {
      throw new ERR_INVALID_STATE('a write to the archive made no progress');
    }
    done += bytesWritten;
  }
}

function writeFdFullySync(fd, buffer, position) {
  let done = 0;
  while (done < buffer.length) {
    const bytesWritten =
      fs.writeSync(fd, buffer, done, buffer.length - done, position + done);
    if (bytesWritten <= 0) {
      throw new ERR_INVALID_STATE('a write to the archive made no progress');
    }
    done += bytesWritten;
  }
}

export default {
  fsOpenAsync,
  fsStatAsync,
  fsLstatAsync,
  fsReadlinkAsync,
  fsCloseAsync,
  fsFstatAsync,
  fsFtruncateAsync,
  readFdFully,
  readFdFullySync,
  writeFdFully,
  writeFdFullySync,
};
export {
  fsOpenAsync,
  fsStatAsync,
  fsLstatAsync,
  fsReadlinkAsync,
  fsCloseAsync,
  fsFstatAsync,
  fsFtruncateAsync,
  readFdFully,
  readFdFullySync,
  writeFdFully,
  writeFdFullySync,
};
"#;
