#!/usr/bin/env python3
"""Ask an X11 window to close the way a window manager does (WM_DELETE_WINDOW).

Usage: x11-close-window.py <window id>
"""
import ctypes, sys
x = ctypes.cdll.LoadLibrary("libX11.so.6")
x.XOpenDisplay.restype = ctypes.c_void_p
x.XInternAtom.restype = ctypes.c_ulong
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.c_void_p]
x.XFlush.argtypes = [ctypes.c_void_p]
d = x.XOpenDisplay(None)
win = int(sys.argv[1])
class ClientMessage(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
                ("format", ctypes.c_int), ("data", ctypes.c_long * 5)]
class XEvent(ctypes.Union):
    _fields_ = [("xclient", ClientMessage), ("pad", ctypes.c_long * 24)]
ev = XEvent()
ev.xclient.type = 33  # ClientMessage
ev.xclient.window = win
ev.xclient.message_type = x.XInternAtom(d, b"WM_PROTOCOLS", 0)
ev.xclient.format = 32
ev.xclient.data[0] = x.XInternAtom(d, b"WM_DELETE_WINDOW", 0)
ev.xclient.data[1] = 0
print("sent", x.XSendEvent(d, win, 0, 0, ctypes.byref(ev)))
x.XFlush(d)
