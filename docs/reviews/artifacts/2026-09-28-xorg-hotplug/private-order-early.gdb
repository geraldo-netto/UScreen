# Copyright (c) 2026 Geraldo Netto
set pagination off
set confirm off
set debuginfod enabled off
set auto-load off
set debug-file-directory old/usr/lib/debug
file old/usr/lib/xorg/Xorg
start
set $key = (DevPrivateKey) calloc(1, sizeof(DevPrivateKeyRec))
print dixRegisterPrivateKey($key, PRIVATE_PIXMAP, 120)
set $pixmap = (PixmapPtr)_dixAllocateScreenObjectWithPrivates(0, sizeof(struct _Pixmap), sizeof(struct _Pixmap), (unsigned long)&((PixmapPtr)0)->devPrivates, PRIVATE_PIXMAP)
print global_keys[PRIVATE_PIXMAP].created
print dixRegisterPrivateKey($key, PRIVATE_PIXMAP, 120)
bt 5
kill
