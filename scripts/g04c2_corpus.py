"""Freeze and validate the network-free G04C2 photographic acceptance corpus."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import tempfile
from fractions import Fraction
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORPUS_ROOT = (
    ROOT
    / "apps"
    / "desktop"
    / "src-tauri"
    / "tests"
    / "fixtures"
    / "g04c2-balanced-corpus"
)
MANIFEST_PATH = CORPUS_ROOT / "corpus-manifest.json"
PAGE_WIDTH = 612
PAGE_HEIGHT = 792
PAGE_MARGIN = 36


EXPECTED = (
    {
        "id": "sunflower-head",
        "filename": "Sunflower_head_2015_G2.jpg",
        "oldid": 1251267196,
        "width": 1280,
        "height": 1640,
        "bytes": 335586,
        "sha256": "65d68804b5aa34fd0235578a1640aa67fc7eba3ea40d2269ebdf5a15e054461c",
        "original_width": 3200,
        "original_height": 4100,
        "shard": "f/f5",
    },
    {
        "id": "folk-architecture",
        "filename": "Folk_Architecture_2015_G07.jpg",
        "oldid": 1250325699,
        "width": 1280,
        "height": 1920,
        "bytes": 573944,
        "sha256": "6d456ef68231e17adf98f6be7e673e3baa887bbefc2f428a037ad11e93bbdcf2",
        "original_width": 3400,
        "original_height": 5100,
        "shard": "e/ea",
    },
    {
        "id": "lviv-church",
        "filename": "Lviv_Church_of_the_Dormition_2015_G1.jpg",
        "oldid": 1098857210,
        "width": 1280,
        "height": 2065,
        "bytes": 390581,
        "sha256": "8e2ac2ca4b813ae997550c9383e782506fa7afddcef9fb805d7f91c424d31cd5",
        "original_width": 3100,
        "original_height": 5000,
        "shard": "7/73",
    },
    {
        "id": "uzh-river",
        "filename": "Uzh_River_near_Chernobyl_2019_G2.jpg",
        "oldid": 1110946301,
        "width": 1280,
        "height": 817,
        "bytes": 294673,
        "sha256": "9701b25bed3fd169109e7ef564bd40f34f7dcec5dfb71c41dd0d85f9bb94eed8",
        "original_width": 5800,
        "original_height": 3700,
        "shard": "f/f3",
    },
    {
        "id": "thorichthys-meeki",
        "filename": "Thorichthys_meeki_2019_G1.jpg",
        "oldid": 1238638582,
        "width": 1280,
        "height": 987,
        "bytes": 301084,
        "sha256": "466643377947c17d8864d4041f550ff7381603d7cdb8510c4082f2a05512c0c6",
        "original_width": 3500,
        "original_height": 2700,
        "shard": "a/a6",
    },
    {
        "id": "fruit-on-plate",
        "filename": "Fruit_on_a_plate_2019_G1.jpg",
        "oldid": 1262411860,
        "width": 1280,
        "height": 1013,
        "bytes": 301998,
        "sha256": "f92dade021e25105fc3404d6cba98386ee30ef22861c7e7f4437b17f187e161b",
        "original_width": 4800,
        "original_height": 3800,
        "shard": "9/98",
    },
)

PREVIOUS_UZH = {
    "dimensions": {"width": 1280, "height": 817},
    "bytes": 296546,
    "sha256": "0fa88acf594e48c5a8e87e588056f66aad4cc00035b655648c37c1b54938e727",
}

PROHIBITED_SUBSTITUTIONS = (
    ("sunflower-head", "Sunflower_head_2015_G1.jpg"),
    ("folk-architecture", "Folk_Architecture_2015_G03.jpg"),
    ("thorichthys-meeki", "Another_Thorichthys_photograph.jpg"),
    ("fruit-on-plate", "Another_fruit_still-life_artwork.jpg"),
)


class CorpusError(RuntimeError):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_manifest(path: Path = MANIFEST_PATH) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def inspect_jpeg(data: bytes) -> tuple[int, int, int, int]:
    """Return width, height, precision and component count after strict framing."""
    if len(data) < 4 or data[:2] != b"\xff\xd8":
        raise CorpusError("JPEG SOI magic is missing")
    position = 2
    frame: tuple[int, int, int, int] | None = None
    while position < len(data):
        if data[position] != 0xFF:
            raise CorpusError(f"JPEG marker expected at byte {position}")
        while position < len(data) and data[position] == 0xFF:
            position += 1
        if position >= len(data):
            raise CorpusError("JPEG ends inside marker fill bytes")
        marker = data[position]
        position += 1
        if marker == 0xD9:
            if position != len(data):
                raise CorpusError("JPEG contains trailing or concatenated data")
            if frame is None:
                raise CorpusError("JPEG has no supported start-of-frame marker")
            return frame
        if marker == 0x01 or 0xD0 <= marker <= 0xD7:
            continue
        if position + 2 > len(data):
            raise CorpusError("JPEG segment length is truncated")
        segment_length = int.from_bytes(data[position : position + 2], "big")
        if segment_length < 2:
            raise CorpusError("JPEG segment has an invalid length")
        segment_start = position + 2
        segment_end = position + segment_length
        if segment_end > len(data):
            raise CorpusError("JPEG segment exceeds the file")
        if marker in {0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF}:
            if segment_length < 8:
                raise CorpusError("JPEG start-of-frame segment is truncated")
            precision = data[segment_start]
            height = int.from_bytes(data[segment_start + 1 : segment_start + 3], "big")
            width = int.from_bytes(data[segment_start + 3 : segment_start + 5], "big")
            components = data[segment_start + 5]
            frame = (width, height, precision, components)
        position = segment_end
        if marker != 0xDA:
            continue
        while position < len(data):
            marker_start = data.find(b"\xff", position)
            if marker_start < 0:
                raise CorpusError("JPEG entropy data has no EOI")
            marker_position = marker_start + 1
            while marker_position < len(data) and data[marker_position] == 0xFF:
                marker_position += 1
            if marker_position >= len(data):
                raise CorpusError("JPEG entropy data ends inside a marker")
            scan_marker = data[marker_position]
            if scan_marker == 0x00 or 0xD0 <= scan_marker <= 0xD7:
                position = marker_position + 1
                continue
            position = marker_start
            break
    raise CorpusError("JPEG EOI is missing")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CorpusError(message)


def validate_manifest(manifest: dict[str, object]) -> list[dict[str, object]]:
    require(manifest.get("schemaVersion") == 1, "manifest schemaVersion must be 1")
    require(
        manifest.get("CORPUS_MODE") == "reviewed-rebaseline",
        "CORPUS_MODE must be reviewed-rebaseline",
    )
    require(manifest.get("retrievalDate") == "2026-08-25", "retrievalDate drifted")
    entries = manifest.get("entries")
    require(isinstance(entries, list) and len(entries) == 6, "manifest must contain six entries")
    ids: list[str] = []
    hashes: list[str] = []
    for entry, expected in zip(entries, EXPECTED, strict=True):
        require(isinstance(entry, dict), "manifest entry is not an object")
        entry_id = entry.get("id")
        filename = entry.get("filename")
        require(entry_id == expected["id"], f"unexpected corpus id: {entry_id}")
        require(filename == expected["filename"], f"{entry_id}: exact filename identity changed")
        require(
            entry.get("assetPath") == f"images/{expected['filename']}",
            f"{entry_id}: asset path does not preserve the exact filename",
        )
        require(entry.get("descriptionPageOldId") == expected["oldid"], f"{entry_id}: oldid changed")
        permanent = entry.get("permanentDescriptionUrl")
        require(
            permanent == f"https://commons.wikimedia.org/w/index.php?title=File:{expected['filename']}&oldid={expected['oldid']}",
            f"{entry_id}: permanent description identity changed",
        )
        require(entry.get("derivativeWidth") == 1280, f"{entry_id}: derivative width is not 1280")
        require(
            entry.get("originalDimensions")
            == {"width": expected["original_width"], "height": expected["original_height"]},
            f"{entry_id}: reported original dimensions changed",
        )
        require(
            entry.get("dimensions") == {"width": expected["width"], "height": expected["height"]},
            f"{entry_id}: dimensions changed",
        )
        require(entry.get("bytes") == expected["bytes"], f"{entry_id}: byte count changed")
        require(entry.get("sha256") == expected["sha256"], f"{entry_id}: SHA-256 changed")
        require(entry.get("mime") == "image/jpeg", f"{entry_id}: MIME is not image/jpeg")
        require(entry.get("author") == {"name": "George Chernilevsky", "userUrl": "https://commons.wikimedia.org/wiki/User:George_Chernilevsky"}, f"{entry_id}: author identity changed")
        license_evidence = entry.get("license")
        require(isinstance(license_evidence, dict), f"{entry_id}: license evidence missing")
        require(license_evidence.get("identifier") == "PD-self", f"{entry_id}: license identifier changed")
        require(license_evidence.get("name") == "Public domain", f"{entry_id}: public-domain evidence missing")
        expected_api = (
            "https://commons.wikimedia.org/w/api.php?action=query&format=json&formatversion=2"
            "&prop=imageinfo&iiprop=url%7Csize%7Cmime%7Cmediatype%7Csha1%7Ctimestamp%7Cextmetadata"
            f"&iiurlwidth=1280&titles=File%3A{expected['filename']}"
        )
        require(entry.get("apiRequest") == expected_api, f"{entry_id}: exact API request changed")
        expected_original = f"https://upload.wikimedia.org/wikipedia/commons/{expected['shard']}/{expected['filename']}"
        expected_binary = (
            f"https://upload.wikimedia.org/wikipedia/commons/thumb/{expected['shard']}/"
            f"{expected['filename']}/1280px-{expected['filename']}"
        )
        require(entry.get("originalUrl") == expected_original, f"{entry_id}: original URL changed")
        require(entry.get("resolvedBinaryUrl") == expected_binary, f"{entry_id}: resolved binary URL changed")
        require(license_evidence.get("evidenceUrl") == permanent, f"{entry_id}: licence evidence page changed")
        if entry_id == "uzh-river":
            require(entry.get("previousFrozenEvidence") == PREVIOUS_UZH, "Uzh previousFrozenEvidence was not preserved exactly")
        else:
            require("previousFrozenEvidence" not in entry, f"{entry_id}: unexpected rebaseline evidence")
        ids.append(str(entry_id))
        hashes.append(str(entry.get("sha256")))
    require(len(set(ids)) == 6, "all six corpus ids must remain distinct")
    require(len(set(hashes)) == 6, "all six corpus hashes must remain distinct")
    return entries


def validate_images(entries: list[dict[str, object]], root: Path = CORPUS_ROOT) -> None:
    for entry in entries:
        entry_id = str(entry["id"])
        path = root / str(entry["assetPath"])
        require(path.is_file(), f"{entry_id}: committed JPEG is missing")
        data = path.read_bytes()
        require(len(data) == entry["bytes"], f"{entry_id}: committed byte count differs")
        require(sha256(data) == entry["sha256"], f"{entry_id}: committed SHA-256 differs")
        width, height, precision, components = inspect_jpeg(data)
        require((width, height) == (entry["dimensions"]["width"], entry["dimensions"]["height"]), f"{entry_id}: decoded dimensions differ")
        require(precision == 8, f"{entry_id}: JPEG is not 8-bit")
        require(components == 3, f"{entry_id}: JPEG is not three-component RGB-compatible data")


def decimal(value: Fraction) -> str:
    result = f"{float(value):.6f}".rstrip("0").rstrip(".")
    return result if result else "0"


def page_content(width: int, height: int) -> bytes:
    available_width = PAGE_WIDTH - 2 * PAGE_MARGIN
    available_height = PAGE_HEIGHT - 2 * PAGE_MARGIN
    scale = min(Fraction(available_width, width), Fraction(available_height, height))
    drawn_width = width * scale
    drawn_height = height * scale
    x = (Fraction(PAGE_WIDTH) - drawn_width) / 2
    y = (Fraction(PAGE_HEIGHT) - drawn_height) / 2
    return (
        "q\n"
        f"{decimal(drawn_width)} 0 0 {decimal(drawn_height)} {decimal(x)} {decimal(y)} cm\n"
        "/Im0 Do\n"
        "Q\n"
    ).encode("ascii")


def build_pdf(entries: list[dict[str, object]], root: Path = CORPUS_ROOT) -> bytes:
    page_objects = [5 + index * 3 for index in range(len(entries))]
    objects: list[bytes] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        f"<< /Type /Pages /Count {len(entries)} /Kids [{' '.join(f'{number} 0 R' for number in page_objects)}] >>".encode("ascii"),
    ]
    for index, entry in enumerate(entries):
        image_data = (root / str(entry["assetPath"])).read_bytes()
        width = int(entry["dimensions"]["width"])
        height = int(entry["dimensions"]["height"])
        image_object = (
            f"<< /Type /XObject /Subtype /Image /Width {width} /Height {height} "
            f"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {len(image_data)} >>\nstream\n"
        ).encode("ascii") + image_data + b"\nendstream"
        content = page_content(width, height)
        content_object = f"<< /Length {len(content)} >>\nstream\n".encode("ascii") + content + b"endstream"
        image_number = 3 + index * 3
        content_number = 4 + index * 3
        page_object = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources << /XObject << /Im0 {image_number} 0 R >> >> /Contents {content_number} 0 R >>"
        ).encode("ascii")
        objects.extend((image_object, content_object, page_object))
    output = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets: list[int] = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(output))
        output.extend(f"{number} 0 obj\n".encode("ascii"))
        output.extend(body)
        output.extend(b"\nendobj\n")
    xref = len(output)
    output.extend(f"xref\n0 {len(objects) + 1}\n".encode("ascii"))
    output.extend(b"0000000000 65535 f \n")
    for offset in offsets:
        output.extend(f"{offset:010d} 00000 n \n".encode("ascii"))
    output.extend(
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode("ascii")
    )
    return bytes(output)


def expected_pdf_inventory(manifest: dict[str, object]) -> list[tuple[str, list[dict[str, object]]]]:
    by_id = {entry["id"]: entry for entry in manifest["entries"]}
    inventory = []
    for pdf in manifest["generatedPdfs"]:
        entries = [by_id[source_id] for source_id in pdf["sourceIds"]]
        require(len(entries) == pdf["pageCount"], f"{pdf['path']}: page inventory mismatch")
        inventory.append((str(pdf["path"]), entries))
    return inventory


def write_generated_pdfs(manifest: dict[str, object], output_root: Path) -> dict[str, str]:
    hashes: dict[str, str] = {}
    for relative_path, entries in expected_pdf_inventory(manifest):
        destination = output_root / Path(relative_path).name
        destination.parent.mkdir(parents=True, exist_ok=True)
        data = build_pdf(entries)
        destination.write_bytes(data)
        hashes[destination.name] = sha256(data)
    return hashes


def extract_dct_streams(pdf: bytes) -> list[tuple[int, int, bytes]]:
    streams: list[tuple[int, int, bytes]] = []
    cursor = 0
    while True:
        marker = pdf.find(b"/Subtype /Image", cursor)
        if marker < 0:
            return streams
        dictionary_start = pdf.rfind(b"<<", 0, marker)
        stream_marker = pdf.find(b"\nstream\n", marker)
        require(dictionary_start >= 0 and stream_marker >= 0, "PDF image stream framing is invalid")
        dictionary = pdf[dictionary_start:stream_marker]
        require(b"/Filter /DCTDecode" in dictionary, "PDF image is not a DCT stream")
        length_match = re.search(rb"/Length (\d+)", dictionary)
        width_match = re.search(rb"/Width (\d+)", dictionary)
        height_match = re.search(rb"/Height (\d+)", dictionary)
        require(length_match is not None and width_match is not None and height_match is not None, "PDF image dictionary is incomplete")
        stream_start = stream_marker + len(b"\nstream\n")
        stream_length = int(length_match.group(1))
        stream = pdf[stream_start : stream_start + stream_length]
        require(pdf[stream_start + stream_length : stream_start + stream_length + len(b"\nendstream")] == b"\nendstream", "PDF image length does not end at endstream")
        streams.append((int(width_match.group(1)), int(height_match.group(1)), stream))
        cursor = stream_start + stream_length


def validate_pdfs(manifest: dict[str, object], pdf_root: Path) -> None:
    for relative_path, entries in expected_pdf_inventory(manifest):
        path = pdf_root / Path(relative_path).name
        require(path.is_file(), f"generated PDF is missing: {path.name}")
        data = path.read_bytes()
        require(data.startswith(b"%PDF-1.4\n"), f"{path.name}: PDF header differs")
        require(data.endswith(b"%%EOF\n"), f"{path.name}: PDF EOF differs")
        require(data.count(b"/Type /Page ") == len(entries), f"{path.name}: page count differs")
        require(b"/CreationDate" not in data and b"/ModDate" not in data and b"/ID" not in data, f"{path.name}: nondeterministic metadata is present")
        streams = extract_dct_streams(data)
        require(len(streams) == len(entries), f"{path.name}: image stream count differs")
        for (actual_width, actual_height, stream), entry in zip(streams, entries, strict=True):
            require((actual_width, actual_height) == (entry["dimensions"]["width"], entry["dimensions"]["height"]), f"{path.name}: embedded image dimensions differ")
            require(len(stream) == entry["bytes"], f"{path.name}: embedded DCT byte count differs")
            require(sha256(stream) == entry["sha256"], f"{path.name}: embedded DCT SHA-256 differs")


def expect_rejection(manifest: dict[str, object], description: str) -> None:
    try:
        validate_manifest(manifest)
    except CorpusError:
        return
    raise CorpusError(f"negative probe was accepted: {description}")


def run_negative_probes(manifest: dict[str, object]) -> None:
    by_id = {entry["id"]: index for index, entry in enumerate(manifest["entries"])}
    for entry_id, wrong_filename in PROHIBITED_SUBSTITUTIONS:
        changed = copy.deepcopy(manifest)
        entry = changed["entries"][by_id[entry_id]]
        entry["filename"] = wrong_filename
        entry["assetPath"] = f"images/{wrong_filename}"
        expect_rejection(changed, f"{entry_id} substituted with {wrong_filename}")
    original_resolution = copy.deepcopy(manifest)
    entry = original_resolution["entries"][0]
    entry["derivativeWidth"] = entry["originalDimensions"]["width"]
    entry["dimensions"] = copy.deepcopy(entry["originalDimensions"])
    expect_rejection(original_resolution, "original-resolution file substituted for 1280px derivative")


def validate_committed() -> tuple[dict[str, object], list[dict[str, object]]]:
    manifest = load_manifest()
    entries = validate_manifest(manifest)
    validate_images(entries)
    validate_pdfs(manifest, CORPUS_ROOT / "pdfs")
    return manifest, entries


def check_all() -> None:
    manifest, _ = validate_committed()
    run_negative_probes(manifest)
    with tempfile.TemporaryDirectory(prefix="document-studio-g04c2-first-") as first_text:
        with tempfile.TemporaryDirectory(prefix="document-studio-g04c2-second-") as second_text:
            first = Path(first_text)
            second = Path(second_text)
            first_hashes = write_generated_pdfs(manifest, first)
            second_hashes = write_generated_pdfs(manifest, second)
            require(first_hashes == second_hashes, "two deterministic generations differ")
            validate_pdfs(manifest, first)
            validate_pdfs(manifest, second)
            for filename in first_hashes:
                committed = (CORPUS_ROOT / "pdfs" / filename).read_bytes()
                require(committed == (first / filename).read_bytes(), f"committed PDF drifted: {filename}")
    print("G04C2 corpus manifest, six exact JPEGs, negative substitutions, deterministic PDFs and unchanged DCT streams verified.")


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("check")
    subparsers.add_parser("validate")
    subparsers.add_parser("negative")
    write_parser = subparsers.add_parser("write")
    write_parser.add_argument("--output", type=Path, default=CORPUS_ROOT / "pdfs")
    args = parser.parse_args()
    if args.command == "check":
        check_all()
    elif args.command == "validate":
        validate_committed()
        print("G04C2 committed corpus validated.")
    elif args.command == "negative":
        run_negative_probes(load_manifest())
        print("G04C2 prohibited-substitution probes rejected.")
    else:
        manifest = load_manifest()
        entries = validate_manifest(manifest)
        validate_images(entries)
        hashes = write_generated_pdfs(manifest, args.output)
        for filename, digest in sorted(hashes.items()):
            print(f"{filename}|{digest}")


if __name__ == "__main__":
    main()
