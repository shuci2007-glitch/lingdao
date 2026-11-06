import ctypes, struct, sys

RT_ICON = 3
RT_GROUP_ICON = 14
LOAD_LIBRARY_AS_DATAFILE = 0x00000002

k = ctypes.windll.kernel32
k.BeginUpdateResourceW.argtypes = [ctypes.c_wchar_p, ctypes.c_bool]
k.BeginUpdateResourceW.restype = ctypes.c_void_p
k.UpdateResourceW.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint16, ctypes.c_void_p, ctypes.c_uint32]
k.UpdateResourceW.restype = ctypes.c_int
k.EndUpdateResourceW.argtypes = [ctypes.c_void_p, ctypes.c_bool]
k.EndUpdateResourceW.restype = ctypes.c_int
k.GetLastError.restype = ctypes.c_uint32

k.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_uint32]
k.LoadLibraryExW.restype = ctypes.c_void_p
k.FindResourceW.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
k.FindResourceW.restype = ctypes.c_void_p
k.LoadResource.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
k.LoadResource.restype = ctypes.c_void_p
k.LockResource.argtypes = [ctypes.c_void_p]
k.LockResource.restype = ctypes.c_void_p
k.SizeofResource.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
k.SizeofResource.restype = ctypes.c_uint32

exe = sys.argv[1]
ico = sys.argv[2]

with open(ico, "rb") as f:
    data = f.read()
assert data[:4] == b"\x00\x00\x01\x00", "not an ICO file"
count = struct.unpack("<H", data[4:6])[0]
print("ico images:", count)

entries = []
off = 6
for i in range(count):
    w, h, colors, resv = data[off], data[off + 1], data[off + 2], data[off + 3]
    planes, bitcount = struct.unpack("<HH", data[off + 4:off + 8])
    size, doff = struct.unpack("<II", data[off + 8:off + 16])
    off += 16
    entries.append((w, h, colors, planes, bitcount, size, data[doff:doff + size]))

# Build RT_GROUP_ICON (GRPICONDIR): reserved(2) type(2) count(2) + entries(14 each)
group = b"\x00\x00" + b"\x01\x00" + struct.pack("<H", count)
for i, (w, h, colors, planes, bitcount, size, _img) in enumerate(entries):
    group += bytes([w, h, colors, 0]) + struct.pack("<HH", planes, bitcount) \
             + struct.pack("<I", size) + struct.pack("<H", i + 1)

h = k.BeginUpdateResourceW(exe, False)
if not h or h == ctypes.c_void_p(-1).value:
    print("BeginUpdateResourceW failed, err =", k.GetLastError())
    sys.exit(1)

for i, (_w, _h, _c, _p, _b, size, img) in enumerate(entries):
    nid = i + 1
    buf = (ctypes.c_char * size)(*img)
    p = ctypes.cast(buf, ctypes.c_void_p)
    if not k.UpdateResourceW(h, ctypes.c_void_p(RT_ICON), ctypes.c_void_p(nid), 0, p, size):
        print(f"UpdateResource RT_ICON {nid} failed, err =", k.GetLastError())
        sys.exit(1)
    print(f"  added RT_ICON id={nid} ({size} bytes)")

gbuf = (ctypes.c_char * len(group))(*group)
gp = ctypes.cast(gbuf, ctypes.c_void_p)
if not k.UpdateResourceW(h, ctypes.c_void_p(RT_GROUP_ICON), ctypes.c_void_p(1), 0, gp, len(group)):
    print("UpdateResource RT_GROUP_ICON failed, err =", k.GetLastError())
    sys.exit(1)
print("  added RT_GROUP_ICON id=1")

if not k.EndUpdateResourceW(h, False):
    print("EndUpdateResourceW failed, err =", k.GetLastError())
    sys.exit(1)
print("ICON INJECTED OK")

# Verify by reading the resource back
hmod = k.LoadLibraryExW(exe, None, LOAD_LIBRARY_AS_DATAFILE)
assert hmod, f"LoadLibraryExW failed err={k.GetLastError()}"
hres = k.FindResourceW(hmod, ctypes.c_void_p(1), ctypes.c_void_p(RT_GROUP_ICON))
assert hres, "RT_GROUP_ICON #1 NOT found after injection!"
size = k.SizeofResource(hmod, hres)
hg = k.LoadResource(hmod, hres)
ptr = k.LockResource(hg)
buf = ctypes.string_at(ptr, size)
vc = struct.unpack("<H", buf[4:6])[0]
print(f"VERIFY: RT_GROUP_ICON present, {size} bytes, contains {vc} icon images")
assert vc == count, "icon count mismatch"
print("ALL GOOD")
