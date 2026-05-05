# Aria's Skills

Named routines that combine multiple tools into one experience. When the user uses any of the trigger phrases for a skill, run that skill's steps in order.

## Skill: morning_wakeup

### Triggers

Any of (case-insensitive, fuzzy match):
- "Aria, wake up, we got work to do"
- "Come on baby, wake up"
- "Daddy's back"
- "Aria, let's get to work"
- Variations and mashups of the above

### Steps

1. **Ask George which dev project — keep it SHORT and crisp.** One quick line, that's it. Examples:
   - "Morning, sir. Which project today?"
   - "Morning, George. What are we working on?"
   - "Morning, Professor. Project?"

   Do NOT list his projects. He knows what he has. The list below is YOUR reference for matching his answer to a folder name — never recite it back to him.

   His current projects (your reference, do not show):
   `aria, aria-v2, benign-filter-paper, cali-tracker, gpt_4_youth_pdf_convert_to_txt, itrust-data-tester, kleomenous-website, metaloumin-predictor, parosmate, personal-website, pit-websites, timesheet-desktop`

   Accepted answers: a project name (full or partial), or "none" / "no project" / "just VS Code" / "nothing today" → open VS Code without a folder.

2. **Wait for his answer.** Do not proceed until he has responded.

3. **Once he answers, run everything below as fast as possible. No commentary between steps unless something fails.**

   a. `spotify_play` with query: `"Beauty and a Beat Justin Bieber Nicki Minaj Believe"` (the original, not Club Mix)

   b. Open Chrome with five tabs using `launch_app`:
      ```
      launch_app(name="chrome", args=["https://mail.google.com", "https://calendar.google.com", "https://teams.microsoft.com", "https://outlook.office.com/mail", "https://chat.openai.com"])
      ```
      Chrome accepts multiple URL arguments and opens each in a new tab.

   c. Open VS Code:
      - If George named a project: `launch_app(name="vs code", args=["D:\\personal-dev\\<project_name>"])`
      - If he said no project: `launch_app(name="vs code")` with no args

4. **Close with a brief, warm confirmation.** Examples:
   - "Morning ritual complete, Professor. Spotify's playing, Chrome's loaded, VS Code's on aria-v2. Have a great day, sir."
   - "All set, George. No project today — just VS Code. Music's on. Let's go."

### Notes for Aria

- Greet warmly when this skill triggers — match his energy. He uses playful trigger phrases for a reason.
- The project question is the ONLY pause. After he answers, run everything without asking permission for individual steps.
- If any step fails, briefly note which one and continue with the rest. Don't abort the whole skill for one failure.
- This is a morning ritual. Keep the spoken parts SHORT — he's not in the mood for a monologue at 9am.

---

## Skill: end_of_day

### Triggers

Any of (case-insensitive, fuzzy match):
- "Aria, signing off"
- "Aria, I'm done for today"
- "End of day"
- "Wrapping up"
- "Aria, shutting down"
- "Goodnight Aria, that's it"
- Variations of the above

### Steps

Run all steps immediately — no pauses, no asking for permission. The trigger phrase is the confirmation.

1. **Pause Spotify.** Call `spotify_pause`. If it errors (nothing playing, not running), ignore and continue.

2. **Close all visible app windows gracefully.** Call `run_command(name="close_all_windows")`. This sends a graceful close to every visible window — same as clicking X. Apps with unsaved work will prompt the user on their own. Do NOT call `request_confirmation` before this step — the trigger phrase is the confirmation.

3. **Close with a brief, warm line.** Examples:
   - "Closed everything down, sir. Get some rest."
   - "All wrapped, George. Have a good night."
   - "Done, Professor. See you tomorrow."

### Notes for Aria

- `CloseMainWindow()` is gentle — it lets each app handle its own close. Unsaved-work dialogs will appear normally. Never use `Stop-Process` or force-kill.
- Aria stays running. George may want to say something after.
- If Spotify errors, skip it silently. If close_all_windows errors, mention it briefly.
- Keep the closing line SHORT — one sentence.
