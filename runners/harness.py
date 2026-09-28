import contextlib
import io
import json
import traceback

# A shared budget bounds the entire result envelope, even for noisy code.
_remaining = 8192


def bounded(text):
    global _remaining
    text = str(text).encode("utf-8", "replace").decode("utf-8")
    kept = text[:min(_remaining, 4096)]
    _remaining -= len(kept)
    return kept + ("\n[output truncated]" if len(kept) < len(text) else "")


class Capture(io.TextIOBase):
    def __init__(self):
        self.parts = []
        self.truncated = False

    def write(self, text):
        if not self.truncated:
            kept = bounded(text)
            self.parts.append(kept)
            self.truncated = kept.endswith("[output truncated]")
        return len(text)

    def value(self):
        return "".join(self.parts)


def run(encoded):
    results = []
    startup_out, startup_err = Capture(), Capture()
    load_error = ""
    with contextlib.redirect_stdout(startup_out), contextlib.redirect_stderr(startup_err):
        try:
            from solution import solve
        except BaseException:
            load_error = bounded(traceback.format_exc(limit=6))
    for index, (name, source) in enumerate(json.loads(encoded)):
        out, err = Capture(), Capture()
        detail = ""
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            if load_error:
                status, detail = "error", load_error
            else:
                try:
                    exec(compile(source, "rep_checks", "exec"), {"solve": solve})
                    status = "passed"
                except AssertionError:
                    status = "failed"
                    detail = bounded(traceback.format_exc(limit=6))
                except BaseException:
                    status = "error"
                    detail = bounded(traceback.format_exc(limit=6))
        results.append({"name": name, "status": status,
                        "stdout": (startup_out.value() if index == 0 else "") + out.value(),
                        "stderr": (startup_err.value() if index == 0 else "") + err.value(),
                        "detail": detail if not load_error or index == 0 else "Module could not load"})
    print("\nSPAR_RESULTS:" + json.dumps(results, ensure_ascii=False))
