/* T575: native adapter bounds, exact connector identity and parser failures. */
#include "../gpu/options.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static void invalid(char **args, int argc) {
    pid_t pid = fork(); assert(pid >= 0);
    if (!pid) { gpu_parse(argc, args); _exit(0); }
    int status; assert(waitpid(pid, &status, 0) == pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 2);
}

static void bounds(char **args) {
    const char *bad[] = {"", "-1", "4294967296", "184467440737095516160", "junk", "3x"};
    for (int i = 3; i <= 9; i++) {
        char *old = args[i];
        for (unsigned j = 0; j < sizeof(bad) / sizeof(bad[0]); j++) {
            args[i] = (char *)bad[j]; invalid(args, 11);
        }
        args[i] = old;
    }
    args[3] = "1279"; invalid(args, 11);
    args[3] = "4096"; args[5] = "2"; invalid(args, 11);
    args[3] = "1280"; args[5] = "1";
    args[10] = "invalid"; invalid(args, 11);
    invalid(args, 10);
}

static void edid(void) {
    unsigned char bytes[513] = {0, 255, 255, 255, 255, 255, 255, 0};
    bytes[127] = 6;
    assert(gpu_edid_valid(bytes, 128));
    for (size_t length = 0; length <= sizeof(bytes); length++) {
        if (length != 128) assert(!gpu_edid_valid(bytes, length));
    }
    for (unsigned i = 0; i < 128; i++) {
        bytes[i] ^= 1; assert(!gpu_edid_valid(bytes, 128)); bytes[i] ^= 1;
    }
    bytes[126] = 1; bytes[127] = 5;
    assert(gpu_edid_valid(bytes, 256));
    bytes[200] = 1; assert(!gpu_edid_valid(bytes, 256));
}

static void transforms(void) {
    int32_t matrix[9] = {65536, 0, 0, 0, 65536, 0, 0, 0, 65536};
    assert(gpu_identity_transform(matrix));
    for (int i = 0; i < 9; i++) {
        matrix[i] ^= 1; assert(!gpu_identity_transform(matrix)); matrix[i] ^= 1;
    }
}

int main(void) {
    char *args[] = {"gpu", "DVI-I-2", "/native/render", "1280", "800", "1", "30", "18", "60000", "0", "desktop"};
    gpu_options options = gpu_parse(11, args);
    assert(options.width == 1280 && options.height == 800 && !options.pattern && options.bitrate == 60000);
    args[10] = "pattern"; assert(gpu_parse(11, args).pattern);
    bounds(args);
    edid();
    transforms();
    assert(gpu_same_render_node(129, 129));
    assert(!gpu_same_render_node(129, 128));
    assert(!gpu_same_render_node(0, 0));
    assert(!gpu_same_render_node(UINT64_MAX, 129));
    assert(gpu_now_ns() > 0);
    return 0;
}
