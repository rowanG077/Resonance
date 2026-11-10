#!/usr/bin/env python3
"""Inspect story-flag setters with retail dialogue and verified original PCs.

First run the decompile_corpus example. Every recovered field flag occurrence is
cross-checked against the word-PC and message-byte identity reports; unsupported expressions fail the audit
instead of silently hiding setters or conflating a companion with one character.
Every occurrence retains its identity domain and resolution status: equal bit
numbers from different fields do not establish a shared meaning. This is a
research aid. Names still require review of all setting paths.
"""
import argparse
import ast
import collections
import csv
import json
from pathlib import Path
import re


def signed(value):
    return (value + 2**31) % 2**32 - 2**31


def constant(source, symbols):
    source = re.sub(r"^\w+:(?!:)\s*", "", source.strip())
    source = re.sub(r"(?:std::)?(\w+::\w+::\w+)",
                    lambda m: str(symbols[m[1]]) if m[1] in symbols else m[0], source)

    def evaluate(node):
        if isinstance(node, ast.Constant) and type(node.value) in (int, bool):
            return signed(int(node.value))
        if isinstance(node, ast.UnaryOp):
            value = evaluate(node.operand)
            if isinstance(node.op, ast.USub):
                return signed(-value)
            if isinstance(node.op, ast.UAdd):
                return value
            if isinstance(node.op, ast.Invert):
                return ~value
        if isinstance(node, ast.BinOp):
            left, right = evaluate(node.left), evaluate(node.right)
            if isinstance(node.op, ast.Add):
                return signed(left + right)
            if isinstance(node.op, ast.Sub):
                return signed(left - right)
            if isinstance(node.op, ast.Mult):
                return signed(left * right)
            if isinstance(node.op, ast.BitOr):
                return left | right
            if isinstance(node.op, ast.BitAnd):
                return left & right
            if isinstance(node.op, ast.BitXor):
                return left ^ right
            if isinstance(node.op, ast.LShift):
                return signed(left << (right & 63)) if right & 63 < 32 else 0
            if isinstance(node.op, ast.RShift):
                return left >> (right & 63) if right & 63 < 32 else (-1 if left < 0 else 0)
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name):
            values = [evaluate(a) for a in node.args]
            if node.func.id in ("s8", "s16", "s32", "u16") and len(values) == 1:
                return values[0]
            if node.func.id == "encoded" and len(values) == 2 and values[0] == values[1]:
                return values[0]
            if (node.func.id == "bound_before" and len(values) == 1
                    and values[0] > -(2**31)):
                return values[0] - 1
            if node.func.id == "flags" and values:
                result = 0
                for value in values:
                    result |= value
                return result
            if (node.func.id == "flag_slot" and len(values) == 2
                    and min(values) >= 0 and sum(values) <= 2**31 - 1):
                return sum(values)
        raise ValueError("not a supported constant")

    try:
        return evaluate(ast.parse(source, mode="eval").body)
    except (ValueError, SyntaxError, KeyError, RecursionError):
        return None


def arguments(source, start):
    depth, previous = 0, start
    result = []
    for at in range(start, len(source)):
        char = source[at]
        if char in "([":
            depth += 1
        elif char in ")]":
            if depth == 0:
                result.append(source[previous:at].strip())
                return result
            depth -= 1
        elif char == "," and depth == 0:
            result.append(source[previous:at].strip())
            previous = at + 1
    raise ValueError("unterminated native arguments")


def definitions(root):
    symbols = {}
    for path in sorted((root / "scripts/std").glob("*.sym")):
        source = code_mask(path.read_text())
        for enumeration in re.finditer(r"pub enum (\w+)\s*\{([^}]+)\}", source, re.S):
            for name, value in re.findall(r"(\w+)\s*=\s*(-?\d+)", enumeration[2]):
                symbols[f"{path.stem}::{enumeration[1]}::{name}"] = int(value)
    return symbols


def text(value, symbols):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "".join(text(v, symbols) for v in value)
    if isinstance(value, dict):
        if "actor_name" in value:
            return f"<{value['actor_name']}>"
        if "item_name" in value:
            names = [k.rsplit("::", 1)[1] for k, v in symbols.items()
                     if k.startswith("items::ItemId::") and v == value["item_name"]]
            return f"<ItemId::{names[0]}>" if names else f"<item {value['item_name']}>"
    return ""


def code_mask(source):
    """Hide strings/comments without changing source positions or line breaks."""
    result = list(source)
    at = 0
    while at < len(source):
        start = at
        if source[at] == '"':
            at += 1
            while at < len(source):
                if source[at] == "\\":
                    at += 2
                elif source[at] == '"':
                    at += 1
                    break
                else:
                    at += 1
        elif source.startswith("//", at):
            at = source.find("\n", at)
            if at == -1:
                at = len(source)
        elif source.startswith("/*", at):
            at += 2
            depth = 1
            while at < len(source) and depth:
                if source.startswith("/*", at):
                    depth += 1
                    at += 2
                elif source.startswith("*/", at):
                    depth -= 1
                    at += 2
                else:
                    at += 1
        else:
            at += 1
            continue
        for index in range(start, min(at, len(source))):
            if source[index] != "\n":
                result[index] = " "
    return "".join(result)


def closing(mask, opening, left="{", right="}"):
    depth = 1
    for at in range(opening + 1, len(mask)):
        if mask[at] == left:
            depth += 1
        elif mask[at] == right:
            depth -= 1
            if depth == 0:
                return at
    raise ValueError("unclosed source block")


def recover(root, assets):
    symbols = definitions(root)
    expected = collections.defaultdict(collections.deque)
    with (root / "identity-uses.tsv").open() as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    for row in sorted(rows, key=lambda r: (r["input"], int(r["pc"], 16))):
        if (row["input"].startswith("fields/")
                and row["domain"] == "story::StoryFlagBase"
                and row["native"] == "bank_reset::clear_event_bit"):
            slot = re.fullmatch(r"bit\[(\d+)\]", row["parameter"])
            if slot is None:
                raise ValueError(f"invalid bank-reset slot reference: {row}")
            value = int(row["value"]) + int(slot[1])
            expected[(row["input"], "clear_event_bit", value)].append(
                dict(row, bank_slot=int(slot[1])))
            continue
        if (row["input"].startswith("fields/")
                and (row["domain"] in ("story::StoryFlag", "story::DebugActorSlotFlag",
                                       "story::DebugLightningFlag")
                     or re.fullmatch(r"story::Field\d{4}Flag", row["domain"]))
                and row["native"] in ("set_event_bit", "clear_event_bit", "test_event_bit")):
            expected[(row["input"], row["native"], int(row["value"]))].append(row)
    message_expected = collections.defaultdict(collections.deque)
    message_report = root / "message-identity-uses.tsv"
    if message_report.exists():
        with message_report.open() as stream:
            messages = list(csv.DictReader(stream, delimiter="\t"))
        for row in sorted(messages, key=lambda r: (r["input"], int(r["byte_offset"], 16))):
            if (row["input"].startswith("fields/")
                    and (row["domain"] == "story::StoryFlag"
                         or re.fullmatch(r"story::Field\d{4}Flag", row["domain"]))
                    and row["native"] in ("set_event_bit", "clear_event_bit", "test_event_bit")):
                message_expected[(row["input"], row["native"], int(row["value"]), int(row["control"]))].append(row)
    result = collections.defaultdict(list)
    call_pattern = re.compile(r"native::(set_event_bit|clear_event_bit|test_event_bit|configure_dialogue)\(")
    for path in sorted(root.glob("fields/*/scripts/scenario.sym")):
        source = path.read_text()
        mask = code_mask(source)
        relative = path.relative_to(root).with_suffix(".ssb").as_posix()
        message_path = assets / path.relative_to(root).parents[1] / "messages.json"
        messages = json.loads(message_path.read_text()) if message_path.exists() else []
        tasks = list(re.finditer(r"^(?:pub )?task (\w+)\(\) \{", mask, re.M))
        for task in tasks:
            end = closing(mask, task.end() - 1) + 1
            body = source[task.start():end]
            body_mask = mask[task.start():end]
            dialogue, flags = [], []
            for call in call_pattern.finditer(body_mask):
                args = arguments(body, call.end())
                if call[1] == "configure_dialogue":
                    for argument in args[6:8]:
                        n = constant(argument, symbols)
                        if n is not None and 0 <= n < len(messages):
                            dialogue.append((call.start(), n, text(messages[n], symbols)))
                else:
                    value = constant(args[0], symbols)
                    if value is None:
                        continue
                    key = (relative, call[1], value)
                    if not expected[key]:
                        raise ValueError(f"source occurrence has no original-PC reference: {key}")
                    reference = expected[key].popleft()
                    flags.append((call.start(), value, call[1], reference, args[0]))
            for position, value, native, reference, expression in flags:
                result[value].append(dict(field=relative.split("/")[1], task=task[1],
                    native=native, pc=reference["pc"], address_space="word_pc", position=position,
                    messages=dialogue, **identity(reference, expression, symbols)))
        labels = list(re.finditer(r"^(\w+):$", mask, re.M))
        for control in re.finditer(r"message_control\(", mask):
            end = closing(mask, control.end() - 1, "(", ")")
            opcode = constant(arguments(source, control.end())[0], symbols)
            body = source[control.end():end]
            body_mask = mask[control.end():end]
            label = next((label[1] for label in reversed(labels) if label.start() < control.start()), "auxiliary")
            slot = int(label[4:], 16) if re.fullmatch(r"aux_[0-9A-F]+", label) else None
            dialogue = [(0, slot, text(messages[slot], symbols))] if slot is not None and slot < len(messages) else []
            for call in call_pattern.finditer(body_mask):
                if call[1] == "configure_dialogue":
                    continue
                expression = arguments(body, call.end())[0]
                value = constant(expression, symbols)
                if value is None:
                    continue
                key = (relative, call[1], value, opcode)
                if not message_expected[key]:
                    raise ValueError(f"source message occurrence has no original-byte reference: {key}")
                reference = message_expected[key].popleft()
                result[value].append(dict(field=relative.split("/")[1], task=label,
                    native=call[1], pc=reference["byte_offset"], address_space="byte", control=opcode,
                    position=call.start(), messages=dialogue,
                    **identity(reference, expression, symbols)))
    missing = [(key, list(pcs)) for key, pcs in expected.items() if pcs]
    if missing:
        raise ValueError(f"unparsed original flag occurrences (regenerate corpus if edited): {missing[:10]}")
    missing_messages = [(key, list(offsets)) for key, offsets in message_expected.items() if offsets]
    if missing_messages:
        raise ValueError(f"unparsed original message flag occurrences: {missing_messages[:10]}")
    return result


def identity(reference, expression, symbols):
    """Preserve report provenance and reject an equal-valued, wrong namespace."""
    domains = {name.rsplit("::", 1)[0]
               for name in re.findall(r"(?:std::)?(\w+::\w+::\w+)", expression)
               if name in symbols}
    if domains and domains != {reference["domain"]}:
        raise ValueError(f"source flag namespace does not match original identity domain: "
                         f"{reference} expression={expression}")
    result = dict(input=reference["input"], domain=reference["domain"],
                  status=reference["status"], expression=expression,
                  identity_value=int(reference["value"]))
    if "bank_slot" in reference:
        result["bank_slot"] = reference["bank_slot"]
    return result


def select(result, *, flag=None, fields=None, domains=None, unresolved=False):
    """Filter only after recovery has audited every original occurrence."""
    for key, selected in [("field", fields), ("domain", domains)]:
        available = {use[key] for uses in result.values() for use in uses}
        if selected and (missing := set(selected) - available):
            raise ValueError(f"unknown {key} values: {', '.join(sorted(missing))}")
    return {value: matches for value, uses in result.items()
            if (flag is None or value == flag)
            and (matches := [use for use in uses
                            if (not fields or use["field"] in fields)
                            and (not domains or use["domain"] in domains)
                            and (not unresolved or use["status"] == "unresolved")])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("flag", nargs="?", type=int)
    parser.add_argument("--corpus", type=Path, default=Path("local/readable-scripts"))
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--output", type=Path, help="write selected cross-checked context as JSON")
    parser.add_argument("--all-uses", action="store_true", help="include readers and clears in a focused flag query")
    parser.add_argument("--field", action="append", dest="fields",
                        help="limit results to this exact field directory name; repeat for several")
    parser.add_argument("--domain", action="append", dest="domains",
                        help="limit results to this exact identity domain; repeat for several")
    parser.add_argument("--unresolved", action="store_true",
                        help="show only occurrences without a supported symbolic identity")
    args = parser.parse_args()
    result = recover(args.corpus, args.assets)
    try:
        result = select(result, flag=args.flag, fields=args.fields, domains=args.domains,
                        unresolved=args.unresolved)
    except ValueError as error:
        parser.error(str(error))
    if args.output:
        args.output.write_text(json.dumps(result, indent=2) + "\n")
    if args.flag is None:
        for value, uses in sorted(result.items()):
            for domain in sorted({use["domain"] for use in uses}):
                scoped = [use for use in uses if use["domain"] == domain]
                fields = sorted({use["field"] for use in scoped if use["native"] == "set_event_bit"})
                statuses = ",".join(sorted({use["status"] for use in scoped}))
                print(value, domain, statuses, len(scoped), ", ".join(fields), sep="\t")
    else:
        for use in result.get(args.flag, []):
            if use["native"] != "set_event_bit" and not args.all_uses:
                continue
            print(f"\n{args.flag} [{use['domain']}, {use['status']}]: {use['field']} "
                  f"{use['task']} @ {use['address_space']} {use['pc']} ({use['native']})")
            print(use["expression"])
            before = [(n, t) for pos, n, t in use["messages"] if pos < use["position"] and t]
            after = [(n, t) for pos, n, t in use["messages"] if pos >= use["position"] and t]
            for number, message in before[-8:] + after[:8]:
                print(f"{number}: {message}")


if __name__ == "__main__":
    main()
