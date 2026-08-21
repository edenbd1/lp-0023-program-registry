// Upload a file to a real Logos Storage node and print its content identifier.
//
// This is the missing half of the IDL criterion: the registry could always
// record and resolve an identifier, but had no way to obtain one, because
// nothing here could talk to Logos Storage. It can now — `libstorage` from
// `logos-storage-nim` runs a node in-process.
//
// Upload is a session rather than a call: init, then file, and the content
// address only exists once the node has really taken the bytes. Printing the CID
// only after `storage_upload_finalize`-equivalent success is what separates "the
// call returned" from "the file is stored".
//
// Exit code is the result. Build with tools/storage-upload.sh.

#include "libstorage.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static volatile int done = 0;
static volatile int code = 0;
static char reply[4096];

static void cb(int ret, const char *msg, size_t len, void *ud) {
  (void)ud;
  if (ret == RET_PROGRESS) return;  // not terminal
  if (msg && len) {
    size_t n = len < sizeof reply - 1 ? len : sizeof reply - 1;
    memcpy(reply, msg, n);
    reply[n] = 0;
  } else {
    reply[0] = 0;
  }
  code = ret;
  done = 1;
}

// Arm before the call, wait after it: the callback can fire before the call
// returns, so clearing the flag afterwards would lose the reply.
static void arm(void) { done = 0; code = -1; reply[0] = 0; }

static int settled(int seconds) {
  for (int i = 0; i < seconds * 20; i++) {
    if (done) return code == RET_OK;
    usleep(50000);
  }
  return 0;
}

int main(int argc, char **argv) {
  if (argc < 2) {
    fprintf(stderr, "usage: storage-upload <file>\n");
    return 2;
  }
  const char *path = argv[1];
  const char *cfg = "{\"log-level\":\"WARN\",\"data-dir\":\"./lp0023-storage\"}";

  arm();
  void *ctx = storage_new(cfg, (StorageCallback)cb, NULL);
  if (!ctx || !settled(30)) {
    fprintf(stderr, "the node was not constructed\n");
    return 1;
  }

  arm();
  storage_start(ctx, (StorageCallback)cb, NULL);
  if (!settled(90)) {
    fprintf(stderr, "the node did not start\n");
    return 1;
  }

  arm();
  storage_upload_init(ctx, path, 0, (StorageCallback)cb, NULL);
  if (!settled(60) || !reply[0]) {
    fprintf(stderr, "upload session was not created\n");
    return 1;
  }
  char session[512];
  snprintf(session, sizeof session, "%s", reply);

  arm();
  storage_upload_file(ctx, session, (StorageCallback)cb, NULL);
  if (!settled(600) || !reply[0]) {
    fprintf(stderr, "the node did not take the bytes\n");
    return 1;
  }
  // Only now is there a content address. Printing anything before this would be
  // printing a session id and calling it a CID.
  // Prefixed, because the node logs to stdout as well and a caller grepping for
  // "a line that looks like a CID" would eventually match one of its log lines.
  printf("CID %s\n", reply);

  arm();
  storage_stop(ctx, (StorageCallback)cb, NULL);
  settled(30);
  // storage_destroy takes only the context: it is not an async call and has no
  // reply to wait for.
  storage_destroy(ctx);
  return 0;
}
