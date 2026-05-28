"""Python source codec — `# coding: pythonjsx` at the top of a `.py` file.

The codec is registered by calling `pythonjsx.codec.register()` once at
application startup.  Then any `.py` file whose *first or second* source
line (see PEP 263) is a coding declaration like

    # coding: pythonjsx
    # -*- coding: pythonjsx -*-

will be decoded through `pythonjsx_decode`.
"""

import codecs
import subprocess

from pythonjsx._compiler_discovery import find_compiler


def pythonjsx_decode(input, errors='strict'):
    if isinstance(input, memoryview):
        input = bytes(input)

    # We invoke the compiler as a subprocess because the codec API expects
    # a string return from a function running during Python's tokenize
    # phase — we can't do it in-process without re-entering the compiler
    # we're in the middle of importing.
    compiler = find_compiler() or "pythonjsx"
    cmd = [compiler, "compile", "-"]

    try:
        process = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        stdout, stderr = process.communicate(input)
        
        if process.returncode != 0:
            raise ValueError(f"pythonjsx compilation failed: {stderr.decode('utf-8', errors='replace')}")
            
        decoded = stdout.decode('utf-8', errors)
        return decoded, len(input)
    except Exception as e:
        raise ValueError(f"Failed to compile pythonjsx: {e}")


class Codec(codecs.Codec):
    def encode(self, input, errors='strict'):
        return codecs.utf_8_encode(input, errors)

    def decode(self, input, errors='strict'):
        return pythonjsx_decode(input, errors)


class IncrementalEncoder(codecs.IncrementalEncoder):
    def encode(self, input, final=False):
        return codecs.utf_8_encode(input, self.errors)[0]


class IncrementalDecoder(codecs.BufferedIncrementalDecoder):
    def _buffer_decode(self, input, errors, final):
        if final:
            return pythonjsx_decode(input, errors)
        else:
            return "", 0


class StreamWriter(Codec, codecs.StreamWriter):
    pass


class StreamReader(Codec, codecs.StreamReader):
    pass


def search_function(encoding):
    if encoding != 'pythonjsx':
        return None
    return codecs.CodecInfo(
        name='pythonjsx',
        encode=Codec().encode,
        decode=Codec().decode,
        incrementalencoder=IncrementalEncoder,
        incrementaldecoder=IncrementalDecoder,
        streamwriter=StreamWriter,
        streamreader=StreamReader,
    )

def register():
    try:
        codecs.lookup('pythonjsx')
    except LookupError:
        codecs.register(search_function)
