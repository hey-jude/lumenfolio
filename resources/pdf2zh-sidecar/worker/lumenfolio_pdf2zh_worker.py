#!/usr/bin/env python3
"""JSON-lines worker for Lumenfolio PDFMathTranslate-next integration."""

from __future__ import annotations

import asyncio
import contextlib
import importlib.metadata
import json
import logging
import os
import sys
import traceback
from pathlib import Path
from typing import Any

SIDECAR_VERSION = "lumenfolio-pdf2zh-sidecar-0.1.0"
PROTOCOL_VERSION = "lumenfolio-pdf2zh-v1"
DEFAULT_PREFLIGHT_TIMEOUT_SECONDS = 8.0
DEFAULT_TRANSLATE_TIMEOUT_SECONDS = 30.0
ORIGINAL_STDOUT_FD = os.dup(sys.stdout.fileno())
ORIGINAL_STDOUT = os.fdopen(
    ORIGINAL_STDOUT_FD,
    "w",
    buffering=1,
    encoding=sys.stdout.encoding or "utf-8",
    closefd=True,
)


def emit(request_id: str, event: str, **payload: Any) -> None:
    message = {"id": request_id, "event": event, **payload}
    ORIGINAL_STDOUT.write(json.dumps(message, ensure_ascii=False, default=str) + "\n")
    ORIGINAL_STDOUT.flush()


def redact(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            key: ("[REDACTED]" if "key" in key.lower() or "token" in key.lower() else redact(item))
            for key, item in value.items()
        }
    if isinstance(value, list):
        return [redact(item) for item in value]
    if isinstance(value, str) and value.startswith("sk-"):
        return "[REDACTED]"
    return value


def setup_logging(log_dir: str | None) -> None:
    handlers: list[logging.Handler] = [logging.StreamHandler(sys.stderr)]
    if log_dir:
        path = Path(log_dir)
        path.mkdir(parents=True, exist_ok=True)
        handlers.append(logging.FileHandler(path / "lumenfolio-pdf2zh-worker.log", encoding="utf-8"))
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
        handlers=handlers,
    )


def request_timeout_seconds(default_value: float) -> float:
    raw_value = os.environ.get("LUMENFOLIO_PDF2ZH_REQUEST_TIMEOUT_SECONDS")
    if not raw_value:
        return default_value
    try:
        value = float(raw_value)
    except ValueError:
        return default_value
    return value if value > 0 else default_value


def install_requests_default_timeout(default_value: float) -> None:
    import requests.sessions

    current = requests.sessions.Session.request
    if getattr(current, "_lumenfolio_timeout_patch", False):
        return

    timeout_seconds = request_timeout_seconds(default_value)

    def request_with_timeout(self, method, url, **kwargs):
        if kwargs.get("timeout") is None:
            kwargs["timeout"] = timeout_seconds
        return current(self, method, url, **kwargs)

    request_with_timeout._lumenfolio_timeout_patch = True  # type: ignore[attr-defined]
    requests.sessions.Session.request = request_with_timeout


@contextlib.contextmanager
def isolate_protocol_stdout():
    sys.stdout.flush()
    sys.stderr.flush()
    saved_stdout_fd = os.dup(sys.stdout.fileno())
    try:
        os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
        with contextlib.redirect_stdout(sys.stderr):
            yield
    finally:
        sys.stdout.flush()
        sys.stderr.flush()
        os.dup2(saved_stdout_fd, sys.stdout.fileno())
        os.close(saved_stdout_fd)


def package_version(package_name: str) -> str:
    try:
        return importlib.metadata.version(package_name)
    except Exception:
        return ""


def ensure_writable(paths: list[str]) -> None:
    for value in paths:
        path = Path(value)
        path.mkdir(parents=True, exist_ok=True)
        probe = path / ".lumenfolio-write-test"
        probe.write_text("ok", encoding="utf-8")
        probe.unlink(missing_ok=True)


def configure_runtime_home(payload: dict[str, Any]) -> None:
    home_dir = Path(payload["cacheDir"]) / "home"
    home_dir.mkdir(parents=True, exist_ok=True)
    os.environ["HOME"] = str(home_dir)
    os.environ["USERPROFILE"] = str(home_dir)
    os.environ.setdefault("XDG_CACHE_HOME", str(Path(payload["cacheDir"]) / "xdg"))


def handle_probe(request_id: str, payload: dict[str, Any]) -> None:
    setup_logging(payload.get("logDir"))
    ensure_writable([
        payload["appDataDir"],
        payload["logDir"],
        payload["cacheDir"],
        payload["artifactsDir"],
        payload["tmpDir"],
    ])
    configure_runtime_home(payload)
    try:
        import pdf2zh_next  # noqa: F401
        import babeldoc  # noqa: F401
    except Exception as exc:
        emit(
            request_id,
            "error",
            status="failed",
            retryable=False,
            errorCode="runtime_import_failed",
            message=f"PDFMathTranslate-next runtime import failed: {exc}",
        )
        return
    emit(
        request_id,
        "probe_result",
        status="succeeded",
        sidecarVersion=SIDECAR_VERSION,
        protocolVersion=PROTOCOL_VERSION,
        pdf2zhVersion=package_version("pdf2zh-next") or package_version("pdf2zh_next"),
        babeldocVersion=package_version("babeldoc"),
        python=sys.executable,
    )


def build_settings(payload: dict[str, Any]):
    from pdf2zh_next.config.model import SettingsModel
    from pdf2zh_next.config.translate_engine_model import BingSettings, GoogleSettings, OpenAISettings

    engine = payload.get("engine") or {}
    provider = (engine.get("provider") or "google-web").lower()
    if provider in {"google", "google-web"}:
        translate_engine_settings = GoogleSettings()
    elif provider in {"bing", "microsoft"}:
        translate_engine_settings = BingSettings()
    elif provider in {"llm", "openai", "openai-compatible"}:
        translate_engine_settings = OpenAISettings(
            openai_api_key=engine.get("apiKey") or os.environ.get("OPENAI_API_KEY"),
            openai_base_url=engine.get("baseUrl") or os.environ.get("OPENAI_BASE_URL"),
            openai_model=engine.get("model") or os.environ.get("OPENAI_MODEL") or "gpt-4o-mini",
        )
    else:
        raise ValueError(f"Unsupported PDF translation provider: {engine.get('provider')}")

    settings = SettingsModel(translate_engine_settings=translate_engine_settings)
    settings.report_interval = 0.25
    settings.translation.lang_in = payload.get("sourceLang") or "en"
    settings.translation.lang_out = payload["targetLang"]
    settings.translation.output = payload["outputDir"]
    settings.translation.ignore_cache = bool(payload.get("forceRefresh"))
    settings.pdf.pages = payload.get("pages") or None
    settings.pdf.only_include_translated_page = bool(payload.get("onlyIncludeTranslatedPage"))
    settings.pdf.watermark_output_mode = "no_watermark"

    artifact_mode = (payload.get("artifactMode") or "both").lower()
    settings.pdf.no_mono = artifact_mode == "dual"
    settings.pdf.no_dual = artifact_mode == "mono"
    return settings


def normalize_progress(event: dict[str, Any]) -> tuple[str, int, int | None]:
    phase = str(event.get("stage") or event.get("phase") or event.get("type") or "translating")
    current_page = event.get("page") or event.get("page_no") or event.get("current_page")
    try:
        current_page = int(current_page) if current_page is not None else None
    except Exception:
        current_page = None

    for key in ("progress", "progress_percent", "percent"):
        if key in event:
            try:
                progress = float(event[key])
                if progress <= 1:
                    progress *= 100
                return phase, max(0, min(99, round(progress))), current_page
            except Exception:
                pass

    finished = event.get("finished") or event.get("completed") or event.get("done")
    total = event.get("total")
    try:
        if finished is not None and total:
            return phase, max(0, min(99, round(float(finished) / float(total) * 100))), current_page
    except Exception:
        pass
    return phase, 1, current_page


def first_requested_page(pages: Any) -> int | None:
    if not pages:
        return None
    first = str(pages).split(",", 1)[0].split("-", 1)[0].strip()
    try:
        page = int(first)
    except Exception:
        return None
    return page or None


async def handle_translate_async(request_id: str, payload: dict[str, Any]) -> None:
    setup_logging(payload.get("logDir"))
    install_requests_default_timeout(DEFAULT_TRANSLATE_TIMEOUT_SECONDS)
    configure_runtime_home(payload)
    logging.info("translate request %s", redact(payload))
    Path(payload["outputDir"]).mkdir(parents=True, exist_ok=True)
    Path(payload["cacheDir"]).mkdir(parents=True, exist_ok=True)

    from pdf2zh_next.high_level import do_translate_async_stream

    settings = build_settings(payload)
    input_pdf = Path(payload["inputPdfPath"])
    emit(request_id, "progress", status="running", phase="starting", progressPercent=1)

    async for event in do_translate_async_stream(settings, input_pdf):
        event_type = event.get("type")
        if event_type == "finish":
            result = event.get("translate_result")
            mono_pdf_path = str(getattr(result, "mono_pdf_path", "") or "")
            dual_pdf_path = str(getattr(result, "dual_pdf_path", "") or "")
            if payload.get("onlyIncludeTranslatedPage"):
                emit(
                    request_id,
                    "artifact_ready",
                    status="partial",
                    phase="partial_ready",
                    progressPercent=35,
                    currentPage=first_requested_page(payload.get("pages")),
                    artifactScope="partial",
                    artifactPages=payload.get("pages") or "",
                    monoPdfPath=mono_pdf_path,
                    dualPdfPath=dual_pdf_path,
                )
            emit(
                request_id,
                "finish",
                status="succeeded",
                phase="finished",
                progressPercent=100,
                monoPdfPath=mono_pdf_path,
                dualPdfPath=dual_pdf_path,
            )
            return
        if event_type == "error":
            emit(
                request_id,
                "error",
                status="failed",
                phase="failed",
                retryable=True,
                errorCode=str(event.get("error_type") or "pdf2zh_error"),
                message=str(event.get("error") or "PDF translation failed"),
            )
            return

        phase, progress, current_page = normalize_progress(event)
        emit(
            request_id,
            "progress",
            status="running",
            phase=phase,
            progressPercent=progress,
            currentPage=current_page,
        )


def handle_translate(request_id: str, payload: dict[str, Any]) -> None:
    asyncio.run(handle_translate_async(request_id, payload))


def handle_preflight(request_id: str, payload: dict[str, Any]) -> None:
    setup_logging(payload.get("logDir"))
    install_requests_default_timeout(DEFAULT_PREFLIGHT_TIMEOUT_SECONDS)
    configure_runtime_home(payload)
    logging.info("preflight request %s", redact(payload))
    Path(payload["outputDir"]).mkdir(parents=True, exist_ok=True)
    Path(payload["cacheDir"]).mkdir(parents=True, exist_ok=True)

    from pdf2zh_next.translator.utils import get_translator

    settings = build_settings(payload)
    get_translator(settings)
    emit(
        request_id,
        "preflight_result",
        status="succeeded",
        phase="preflight_ready",
        progressPercent=0,
    )


def handle_request(request: dict[str, Any]) -> None:
    request_id = request.get("id") or "unknown"
    if request.get("protocolVersion") != PROTOCOL_VERSION:
        emit(
            request_id,
            "error",
            status="failed",
            retryable=False,
            errorCode="protocol_mismatch",
            message="Unsupported sidecar protocol version",
        )
        return
    kind = request.get("kind")
    payload = request.get("payload") or {}
    if kind == "probe":
        handle_probe(request_id, payload)
    elif kind == "preflight":
        handle_preflight(request_id, payload)
    elif kind == "translate":
        handle_translate(request_id, payload)
    else:
        emit(
            request_id,
            "error",
            status="failed",
            retryable=False,
            errorCode="unknown_request",
            message=f"Unknown sidecar request kind: {kind}",
        )


def main() -> int:
    line = sys.stdin.readline()
    if not line:
        return 0
    try:
        request = json.loads(line)
        with isolate_protocol_stdout():
            handle_request(request)
        return 0
    except Exception as exc:
        request_id = "unknown"
        with contextlib.suppress(Exception):
            request_id = request.get("id") or "unknown"  # type: ignore[name-defined]
        setup_logging(os.environ.get("LUMENFOLIO_PDF2ZH_LOG_DIR"))
        logging.error("Unhandled sidecar error: %s\n%s", exc, traceback.format_exc())
        emit(
            request_id,
            "error",
            status="failed",
            phase="failed",
            retryable=True,
            errorCode=exc.__class__.__name__,
            message=str(exc),
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
