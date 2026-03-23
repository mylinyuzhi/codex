<command-name>loop</command-name>

# Loop Command

You are handling the `/loop` command. This command creates a repeating scheduled task that fires on a recurring interval.

## Syntax

`/loop [interval] <prompt-or-command>`

Where:
- `[interval]` is an optional duration like `5m`, `1h`, `30s`, `2d` (default: `10m`)
- `<prompt-or-command>` is the command or prompt to execute repeatedly

## Behavior

1. Parse the interval and prompt from the user's arguments
2. Convert the interval to a cron expression:
   - `5m` -> cron: `*/5 * * * *`
   - `1h` -> cron: `0 */1 * * *`
   - `30s` -> cron: `*/1 * * * *` (minimum granularity is 1 minute)
   - `2d` -> cron: `0 0 */2 * *`
   - `10m` -> cron: `*/10 * * * *` (default if no interval specified)
3. Call the CronCreate tool with `recurring: true` and the computed cron expression
4. Report the created job ID and human-readable schedule to the user

## Examples

- `/loop 5m check git status` -> CronCreate with cron="*/5 * * * *", prompt="check git status", recurring=true
- `/loop 1h run the test suite` -> CronCreate with cron="0 */1 * * *", prompt="run the test suite", recurring=true
- `/loop check for new issues` -> CronCreate with cron="*/10 * * * *", prompt="check for new issues", recurring=true (default 10m)

## No Arguments

If the user runs `/loop` with no arguments, call CronList to show all active scheduled jobs.

## Important

- Always set `recurring: true` (this is the default)
- Do NOT set `durable: true` unless the user explicitly asks for persistence across sessions
- After creating the job, confirm with the user what was scheduled and at what interval
