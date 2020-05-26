import socket
import select
import time
from threading import Thread
import signal
import sys

tello_ip = "192.168.10.1"
tello_video = (tello_ip, 11111)

class TelloCommand:
    # timeout in seconds for a command
    # num_retries number of times to retry command
    def __init__(self, ip = "192.168.10.1", port = 8889, timeout = 0.5, num_retries = 5):
        self.timeout = timeout
        self.num_retries = num_retries
        self.tello_cmd = (ip, port)
        self.sock = None
    
    def connect(self):
        if self.sock is not None:
            raise RuntimeError("Command has already been connected!")

        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        if self.__cmd_with_ack("command") != "ok":
            raise RuntimeError("Failed to command Tello :-(")
    
    def get_battery(self):
        return self.__cmd_with_ack("battery?")

    # Send a command to Tello, and retry if it fails
    def __cmd_with_ack(self, command):
        for _i in range(self.num_retries):
            self.sock.sendto(command.encode("utf-8"), self.tello_cmd)
            ready = select.select([self.sock], [], [], self.timeout)
            if ready[0]:
                data, addr = self.sock.recvfrom(4096)
                print(data)
                print(addr)
                return data.decode("utf-8")
        return False

class TelloState:
    def __init__(self, ip = "0.0.0.0", port = 8890):
        self.tello_state = (ip, port)
        self.running = False
        self.data = {}
        self.thread = None

    def battery(self):
        if "bat" not in self.data:
            return -1
        return int(self.data["bat"])

    def close(self):
        self.running = False
        self.thread.join()

    def listen(self):
        self.running = True

        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(self.tello_state)

        self.thread = Thread(target = self.__listen)
        self.thread.start()

    def __listen(self):
        while self.running:
            ready = select.select([self.sock], [], [], 0.5)
            if ready[0]:
                data = self.sock.recv(4096).decode('utf-8')
                for var in data.split(";"):
                    parts = var.split(":")
                    if len(parts) == 2:
                        self.data[parts[0]] = parts[1]


tello_cmd = TelloCommand()
tello_cmd.connect()

tello_state = TelloState()
tello_state.listen()

still_running = True
def update_tello_state_display():
    while still_running:
        print("Battery: {}%".format(tello_state.battery()))
        time.sleep(1)


tello_state_thread = Thread(target = update_tello_state_display)
tello_state_thread.start()

def signal_handler(sig, frame):
    global still_running
    still_running = False

    print("Shutting down...")
    tello_state_thread.join()
    tello_state.close()
    sys.exit(0)

signal.signal(signal.SIGINT, signal_handler)
signal.pause()
