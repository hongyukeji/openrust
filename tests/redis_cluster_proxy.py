"""Loopback relay that discards one acknowledged write in the cluster fixture."""

import asyncio
from contextlib import suppress
from pathlib import Path
import sys


def request(*args):
    return (f"*{len(args)}\r\n" + "".join(f"${len(arg)}\r\n{arg}\r\n" for arg in args)).encode()


COMMAND = request("INCR", "fixture:{two}:lost-reply")
ACK = b":1\r\n"


async def main(port, upstream, directory):
    dropped = False

    async def relay(client_reader, client_writer):
        nonlocal dropped
        try:
            redis_reader, redis_writer = await asyncio.open_connection("127.0.0.1", upstream)
        except OSError:
            client_writer.close()
            return
        armed = False

        async def requests():
            nonlocal armed
            tail = b""
            while data := await client_reader.read(65536):
                chunk = tail + data
                if not dropped and COMMAND in chunk:
                    armed = True
                tail = chunk[-(len(COMMAND) - 1):]
                redis_writer.write(data)
                await redis_writer.drain()

        async def replies():
            nonlocal dropped
            while data := await redis_reader.read(65536):
                if armed and not dropped:
                    if len(data) < len(ACK) and ACK.startswith(data):
                        data += await redis_reader.readexactly(len(ACK) - len(data))
                    if data.startswith(ACK):
                        dropped = True
                        (directory / "acknowledgment-dropped").touch()
                        return
                client_writer.write(data)
                await client_writer.drain()

        tasks = [asyncio.create_task(requests()), asyncio.create_task(replies())]
        try:
            await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            for writer in (redis_writer, client_writer):
                writer.close()
                with suppress(OSError):
                    await writer.wait_closed()

    async with await asyncio.start_server(relay, "127.0.0.1", port) as server:
        (directory / "ready").touch()
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main(int(sys.argv[1]), int(sys.argv[2]), Path(sys.argv[3])))
