import psycopg2
import asyncpg
import asyncio

async def test_asyncpg():
	conn = await asyncpg.connect(
		user='pgdog',
		password='pgdog',
		database='pgdog',
		host='127.0.0.1',
		port=6432,
		statement_cache_size=0)
	for i in range(100):
		values = await conn.fetch("SELECT $1::int, $2::text", 1, "1")
	await conn.close()

async def test_sharded():
    conn = await asyncpg.connect(
		user='pgdog',
		password='pgdog',
		database='pgdog_sharded',
		host='127.0.0.1',
		port=6432,
		statement_cache_size=0)
    for v in range(1):
        values = await conn.fetch("SELECT * FROM sharded WHERE id = $1", v)
    await conn.close()

# asyncio.run(test_asyncpg())
asyncio.run(test_sharded())
