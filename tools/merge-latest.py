#!/usr/bin/env python3
"""合并自动更新清单（latest.json），按平台 key 回写。

Tauri 的 latest.json 结构是「单个 version + platforms 字典」。同一版本在 Linux 与
Windows 分别构建、分别发布时，不能直接用后发布平台的清单覆盖前一个，否则先发布的
平台会从清单里消失。本脚本负责读旧清单、只更新当前平台条目、其余平台保留。

用法:
    merge-latest.py --existing old.json --output new.json \
        --version 1.0.9 --notes "..." --tag v1.0.9 \
        --platform linux-x86_64 \
        --artifact-url https://.../AuroraDB_1.0.9_amd64.AppImage \
        --signature-file AuroraDB.AppImage.sig \
        [--deb-url https://.../AuroraDB_1.0.9_amd64.deb] \
        [--deb-signature-file AuroraDB.deb.sig]

--existing 可为：Gitee contents API 的响应（含 content 字段，自动 base64 解码）、
纯 latest.json、空文件，或文件不存在（按全新清单处理）。
"""

import argparse
import base64
import datetime
import json
import os
import sys


def parse_version(value):
    parts = value.strip().split('.')
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        return None
    return tuple(int(p) for p in parts)


def load_existing(path):
    """读取旧清单；兼容 Gitee API 响应与纯 JSON，读不到时返回 None。"""
    if not path or not os.path.exists(path):
        return None
    with open(path, encoding='utf-8') as f:
        raw = f.read().strip()
    if not raw or raw == 'null':
        return None
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        return None
    if not isinstance(data, dict):
        return None
    # Gitee contents API：正文在 content 字段里，base64 编码
    if 'content' in data and 'platforms' not in data:
        try:
            decoded = base64.b64decode(data['content']).decode('utf-8')
            data = json.loads(decoded)
        except (ValueError, json.JSONDecodeError):
            return None
        if not isinstance(data, dict):
            return None
    if 'platforms' not in data:
        return None
    return data


def same_release(url, tag):
    return isinstance(url, str) and f'/download/{tag}/' in url


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--existing')
    parser.add_argument('--output', required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--notes', required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--platform', required=True)
    parser.add_argument('--artifact-url', required=True)
    parser.add_argument('--signature-file', required=True)
    parser.add_argument('--deb-url')
    parser.add_argument('--deb-signature-file')
    args = parser.parse_args()

    version = parse_version(args.version)
    if version is None:
        sys.exit(f'版本号格式需为 X.Y.Z，当前为 {args.version!r}')

    with open(args.signature_file, encoding='utf-8') as f:
        signature = f.read().strip()
    if not signature:
        sys.exit(f'签名文件为空: {args.signature_file}')

    existing = load_existing(args.existing)
    platforms = {}
    previous = None
    if existing:
        old_version = str(existing.get('version', ''))
        old_parsed = parse_version(old_version)
        if old_parsed and old_parsed > version:
            sys.exit(f'线上清单版本 {old_version} 高于待发布版本 {args.version}，拒绝降级发布')
        # 只保留本次 tag 的条目：旧版本平台的地址指向旧安装包，
        # 混进新版本清单会让对应平台反复收到「同版本更新」提示
        for key, entry in (existing.get('platforms') or {}).items():
            if isinstance(entry, dict) and same_release(entry.get('url'), args.tag):
                platforms[key] = entry
        if old_version and old_version != args.version:
            previous = {
                'version': old_version,
                'platforms': existing.get('platforms') or {},
            }
    platforms[args.platform] = {'signature': signature, 'url': args.artifact_url}

    data = {
        'version': args.version,
        'notes': args.notes,
        'pub_date': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
        'platforms': platforms,
    }
    if previous:
        data['previous'] = previous

    if args.deb_url and same_release(args.deb_url, args.tag):
        deb = {'url': args.deb_url}
        if args.deb_signature_file:
            with open(args.deb_signature_file, encoding='utf-8') as f:
                deb['signature'] = f.read().strip()
        data['deb'] = deb

    with open(args.output, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.write('\n')

    kept = sorted(k for k in platforms if k != args.platform)
    print(f'  清单平台: {args.platform}' + (f'（保留 {", ".join(kept)}）' if kept else '（无其他平台）'))


if __name__ == '__main__':
    main()
