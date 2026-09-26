/* T497: a private-process syscall fixture. No real uinput descriptor is opened. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/uinput.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

int blent_test_uinput_active(void) { return 497; }

static int open_fixture(int directory, const char *path, int flags, mode_t mode) {
    if (!strcmp(path, "/dev/uinput")) {
        path = getenv("BLENT_T497_UINPUT_FILE");
        if (!path) { errno = ENOENT; return -1; }
    }
    return syscall(SYS_openat, directory, path, flags, mode);
}

static mode_t creation_mode(int flags, va_list arguments) {
    if ((flags & O_CREAT) || (flags & O_TMPFILE) == O_TMPFILE)
        return va_arg(arguments, mode_t);
    return 0;
}

int open(const char *path, int flags, ...) {
    va_list arguments;
    va_start(arguments, flags);
    mode_t mode = creation_mode(flags, arguments);
    va_end(arguments);
    return open_fixture(AT_FDCWD, path, flags, mode);
}

int open64(const char *path, int flags, ...) {
    va_list arguments;
    va_start(arguments, flags);
    mode_t mode = creation_mode(flags, arguments);
    va_end(arguments);
    return open_fixture(AT_FDCWD, path, flags, mode);
}

int openat(int directory, const char *path, int flags, ...) {
    va_list arguments;
    va_start(arguments, flags);
    mode_t mode = creation_mode(flags, arguments);
    va_end(arguments);
    return open_fixture(directory, path, flags, mode);
}

int openat64(int directory, const char *path, int flags, ...) {
    va_list arguments;
    va_start(arguments, flags);
    mode_t mode = creation_mode(flags, arguments);
    va_end(arguments);
    return open_fixture(directory, path, flags, mode);
}

static int owned_descriptor(int fd) {
    const char *path = getenv("BLENT_T497_UINPUT_FILE");
    struct stat actual, expected;
    return path && !stat(path, &expected) && !fstat(fd, &actual) &&
        actual.st_dev == expected.st_dev && actual.st_ino == expected.st_ino;
}

static void record(FILE *log, unsigned long request, uintptr_t argument) {
    fprintf(log, "request %lu\n", request);
    if (request == UI_DEV_SETUP) {
        const struct uinput_setup *setup = (const void *)argument;
        fprintf(log, "device %u %u %u %u %zu\n", setup->id.bustype, setup->id.vendor,
            setup->id.product, setup->id.version, strnlen(setup->name, sizeof(setup->name)));
    } else if (request == UI_ABS_SETUP) {
        const struct uinput_abs_setup *setup = (const void *)argument;
        fprintf(log, "axis %u %d %d %d\n", setup->code, setup->absinfo.minimum,
            setup->absinfo.maximum, setup->absinfo.resolution);
    } else if (request != UI_DEV_CREATE && request != UI_DEV_DESTROY) {
        fprintf(log, "value %lu %d\n", request, (int)argument);
    }
}

static int fake_ioctl(int fd, unsigned long request, uintptr_t argument) {
    if (!owned_descriptor(fd)) { errno = EBADF; return -1; }
    const char *failure = getenv("BLENT_T497_UINPUT_FAIL");
    if (failure && strtoul(failure, NULL, 10) == request) { errno = EINVAL; return -1; }
    const char *path = getenv("BLENT_T497_UINPUT_LOG");
    if (!path) { errno = ENOENT; return -1; }
    FILE *log = fopen(path, "a");
    if (!log) return -1;
    record(log, request, argument);
    return fclose(log);
}

int ioctl(int fd, unsigned long request, ...) {
    /* CREATE/DESTROY have no third argument. Other fixture calls supply one. */
    uintptr_t argument = 0;
    if (request != UI_DEV_CREATE && request != UI_DEV_DESTROY) {
        va_list arguments;
        va_start(arguments, request);
        argument = va_arg(arguments, uintptr_t);
        va_end(arguments);
    }
    if (_IOC_TYPE(request) == 'U') return fake_ioctl(fd, request, argument);
    return syscall(SYS_ioctl, fd, request, argument);
}
