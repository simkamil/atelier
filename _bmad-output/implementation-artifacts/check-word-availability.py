# SPDX-License-Identifier: AGPL-3.0-or-later
"""Read Word's COM registration without starting Word or PowerShell."""
import ctypes
import json
import platform
from pathlib import Path

if platform.system() != 'Windows':
    raise SystemExit('This diagnostic requires the Windows COM registry')

guid = (ctypes.c_ubyte * 16)()
ole32 = ctypes.WinDLL('ole32')
ole32.CLSIDFromProgID.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p]
ole32.CLSIDFromProgID.restype = ctypes.c_long
status = ole32.CLSIDFromProgID('Word.Application', ctypes.byref(guid))
registered = status == 0
report = {'date': '2026-10-12', 'method': 'ole32.CLSIDFromProgID via Python ctypes',
          'prog_id': 'Word.Application', 'hresult': f'0x{status & 0xffffffff:08x}',
          'com_registered': registered, 'word_started': False,
          'layout_results': None, 'word_95_percent_established': False}
path = Path(__file__).with_name('us-210-word-com-check.json')
path.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8', newline='\n')
print(json.dumps(report))
