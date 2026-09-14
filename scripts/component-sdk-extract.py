"""只提取已校验 LunarG Qt 安装器的嵌入 7z 包；不运行安装脚本或修改系统。"""
import pathlib
import struct
import subprocess
import sys
import tempfile
import zlib


def extract(installer: pathlib.Path, destination: pathlib.Path, seven_zip: str) -> None:
    """验证 7z 起始头 CRC 与边界，逐个解包 Qt 多载荷容器，避免只提取 Bin。"""
    data = installer.read_bytes()
    signature = bytes.fromhex("377abcaf271c")
    offset = 0
    count = 0
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="quailcard-sdk-") as work:
        while True:
            offset = data.find(signature, offset)
            if offset < 0:
                break
            start = offset
            offset += len(signature)
            if start + 32 > len(data):
                continue
            crc = struct.unpack_from("<I", data, start + 8)[0]
            if zlib.crc32(data[start + 12:start + 32]) != crc:
                continue
            next_offset, next_size = struct.unpack_from("<QQ", data, start + 12)
            end = start + 32 + next_offset + next_size
            if end > len(data):
                continue
            archive = pathlib.Path(work) / "payload.7z"
            archive.write_bytes(data[start:end])
            subprocess.run([seven_zip, "x", str(archive), "-o" + str(destination), "-y"], check=True)
            count += 1
            offset = end
    required = ["Bin/glslc.exe", "Include/vulkan/vulkan.hpp", "Lib/vulkan-1.lib"]
    if not all((destination / name).is_file() for name in required):
        raise RuntimeError("SDK 缺少 Vulkan 编译所需文件")
    print(f"Extracted {count} verified 7z payloads (no installer execution)")


if __name__ == "__main__":
    extract(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3])
