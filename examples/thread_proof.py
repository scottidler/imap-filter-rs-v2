#!/usr/bin/env python3
"""
Proof-of-concept: Demonstrate that Gmail X-GM-THRID can be extracted
and used for thread grouping via IMAP.

Run with:
    IMAP_USERNAME=you@gmail.com IMAP_PASSWORD=your-app-password python3 examples/thread_proof.py

Requirements:
    pip install imapclient  # or just use stdlib imaplib
"""

import imaplib
import os
import re
import email
from collections import defaultdict

def main():
    # Get credentials from environment
    domain = os.environ.get("IMAP_DOMAIN", "imap.gmail.com")
    username = os.environ.get("IMAP_USERNAME")
    password = os.environ.get("IMAP_PASSWORD")
    mailbox = os.environ.get("IMAP_MAILBOX", "INBOX")
    limit = int(os.environ.get("LIMIT", "50"))

    if not username or not password:
        print("Error: IMAP_USERNAME and IMAP_PASSWORD environment variables required")
        print()
        print("Usage:")
        print("  IMAP_USERNAME=you@gmail.com IMAP_PASSWORD=your-app-password python3 examples/thread_proof.py")
        return 1

    print("=== Gmail Thread ID (X-GM-THRID) Proof of Concept (Python) ===\n")
    print(f"Connecting to {domain}...")

    # Connect with SSL
    imap = imaplib.IMAP4_SSL(domain)

    # Login
    imap.login(username, password)
    print(f"✅ Logged in as {username}")

    # Select mailbox
    status, data = imap.select(mailbox)
    if status != "OK":
        print(f"Failed to select {mailbox}")
        return 1

    num_messages = int(data[0])
    print(f"📬 Selected '{mailbox}' - {num_messages} messages\n")

    if num_messages == 0:
        print("No messages in mailbox.")
        imap.logout()
        return 0

    # Calculate range for recent messages
    start = max(1, num_messages - limit + 1)
    msg_range = f"{start}:{num_messages}"

    print(f"Fetching messages {msg_range} with X-GM-THRID...\n")

    # KEY: Fetch X-GM-THRID along with other data
    # This is a Gmail IMAP extension
    status, data = imap.fetch(
        msg_range.encode(),
        "(UID X-GM-THRID X-GM-MSGID BODY.PEEK[HEADER.FIELDS (SUBJECT FROM DATE)])"
    )

    if status != "OK":
        print(f"Fetch failed: {status}")
        return 1

    # Parse responses
    messages = []
    thread_extraction_success = 0
    thread_extraction_failed = 0

    # IMAP fetch returns pairs of (metadata, body) for each message
    i = 0
    while i < len(data):
        item = data[i]
        if isinstance(item, tuple):
            metadata, body = item
            metadata_str = metadata.decode("utf-8", errors="replace")

            # Extract UID
            uid_match = re.search(r"UID (\d+)", metadata_str)
            uid = uid_match.group(1) if uid_match else "?"

            # Extract X-GM-THRID (the thread ID we're proving exists!)
            thrid_match = re.search(r"X-GM-THRID (\d+)", metadata_str)
            thread_id = thrid_match.group(1) if thrid_match else None

            # Extract X-GM-MSGID
            msgid_match = re.search(r"X-GM-MSGID (\d+)", metadata_str)
            msg_id = msgid_match.group(1) if msgid_match else None

            if thread_id:
                thread_extraction_success += 1
            else:
                thread_extraction_failed += 1

            # Parse headers from body
            if isinstance(body, bytes):
                headers = email.message_from_bytes(body)
                subject = headers.get("Subject", "")
                from_addr = headers.get("From", "")
                date = headers.get("Date", "")
            else:
                subject = from_addr = date = ""

            messages.append({
                "uid": uid,
                "thread_id": thread_id,
                "msg_id": msg_id,
                "subject": subject,
                "from": from_addr,
                "date": date,
            })
        i += 1

    # Report extraction results
    print("=== X-GM-THRID Extraction Results ===")
    print(f"✅ Successfully extracted: {thread_extraction_success}")
    print(f"❌ Failed to extract: {thread_extraction_failed}")
    print()

    # ALWAYS show sample raw response for debugging
    print("=== Sample Raw IMAP FETCH Response (for debugging) ===")
    if data and len(data) > 0:
        for i, item in enumerate(data[:4]):  # Show first 2 messages (metadata+body pairs)
            print(f"[{i}] {type(item).__name__}: {repr(item)[:500]}")
    print()

    if thread_extraction_success == 0:
        print("⚠️  WARNING: No thread IDs were extracted!")
        print("   This might mean:")
        print("   1. Gmail extensions aren't enabled for this account")
        print("   2. You're not connected to Gmail (X-GM-THRID is Gmail-specific)")
        print()
    else:
        # Group by thread ID
        threads = defaultdict(list)
        no_thread = []

        for msg in messages:
            if msg["thread_id"]:
                threads[msg["thread_id"]].append(msg)
            else:
                no_thread.append(msg)

        # Find threads with multiple messages
        multi_message_threads = [(tid, msgs) for tid, msgs in threads.items() if len(msgs) > 1]

        print("=== Thread Grouping Results ===")
        print(f"Total unique threads: {len(threads)}")
        print(f"Threads with multiple messages: {len(multi_message_threads)}")
        print(f"Messages without thread ID: {len(no_thread)}")
        print()

        # Show example multi-message threads
        if multi_message_threads:
            print("=== Example Multi-Message Threads ===")
            print("(This proves thread grouping works for state-filter transitions)\n")

            for i, (thread_id, msgs) in enumerate(multi_message_threads[:3]):
                print(f"Thread #{i + 1} (ID: {thread_id})")
                print(f"  Messages in thread: {len(msgs)}")

                for j, msg in enumerate(msgs):
                    print(f"  [{j + 1}] UID: {msg['uid']}")
                    print(f"      Subject: {msg['subject'][:60]}...")
                    print(f"      From: {msg['from'][:40]}")
                    print(f"      Date: {msg['date']}")
                print()

            print("=== PROOF: State-Filter Thread Transitions Are Achievable ===")
            print()
            print("Since we can:")
            print("  1. ✅ Extract X-GM-THRID from Gmail IMAP responses")
            print("  2. ✅ Group messages by thread ID")
            print("  3. ✅ Identify all messages belonging to the same conversation")
            print()
            print("We CAN implement state-filter transitions that operate on entire threads:")
            print("  - If ANY message in a thread is Starred → protect the whole thread")
            print("  - If the NEWEST message in a thread expires → expire the whole thread")
            print("  - Apply TTL based on the most recent message in the thread")
            print()
        else:
            print("No multi-message threads found in the sample.")
            print("Try with a larger LIMIT or a different mailbox with conversation threads.")

    # Logout
    imap.logout()
    print("✅ Done.")
    return 0


if __name__ == "__main__":
    exit(main())

