import socket
import select
import time
from threading import Thread
import signal
import sys
import logging
from cli_ui import TelloCliUi
from ps4_controller import PS4Controller

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
    
    def takeoff(self):
        return self.__cmd_with_ack("takeoff") == "ok"
    
    def land(self):
        return self.__cmd_with_ack("land") == "ok"

    def up(self, x):
        return self.command_with_value("up", x)
    
    def down(self, x):
        return self.command_with_value("down", x)
    
    def left(self, x):
        return self.command_with_value("left", x)
    
    def right(self, x):
        return self.command_with_value("right", x)
    
    def forward(self, x):
        return self.command_with_value("forward", x)
    
    def backward(self, x):
        return self.command_with_value("back", x)
    
    def clockwise(self, x):
        return self.command_with_value("cw", x)
    
    def counter_clockwise(self, x):
        return self.command_with_value("ccw", x)

    def get_battery(self):
        return self.__cmd_with_ack("battery?")

    # Send a command to Tello, and retry if it fails
    def __cmd_with_ack(self, command):
        for _i in range(self.num_retries):
            self.sock.sendto(command.encode("utf-8"), self.tello_cmd)
            ready = select.select([self.sock], [], [], self.timeout)
            if ready[0]:
                data, addr = self.sock.recvfrom(4096)
                return data.decode("utf-8")
        return False
    
    def command_with_value(self, cmd, x):
        return self.__check_ok(cmd, self.__cmd_with_ack("{} {}".format(cmd, x)))

    @staticmethod
    def __check_ok(cmd, val):
        if val == "ok":
            return True
        else:
            logging.error("Failed {} command: {}".format(cmd, val))
            return False

class TelloState:
    def __init__(self, ip = "0.0.0.0", port = 8890):
        self.tello_state = (ip, port)
        self.running = False
        self.data = {}
        self.thread = None

    def pitch(self):
        return self.__int_value("pitch")
    
    def roll(self):
        return self.__int_value("roll")
    
    def yaw(self):
        return self.__int_value("yaw")
    
    def velocity_x(self):
        return self.__int_value("vgx")
    
    def velocity_y(self):
        return self.__int_value("vgy")
    
    def velocity_z(self):
        return self.__int_value("vgz")

    def temp_low(self):
        return self.__int_value("templ")
    
    def temp_high(self):
        return self.__int_value("temph")

    def tof(self):
        return self.__int_value("tof")
    
    def height(self):
        return self.__int_value("h")

    def battery(self):
        return self.__int_value("bat")
    
    def barometer(self):
        return self.__float_value("baro")

    def time(self):
        return self.__int_value("time")
    
    def acceleration_x(self):
        return self.__float_value("agx")
    
    def acceleration_y(self):
        return self.__float_value("agy")
    
    def acceleration_z(self):
        return self.__float_value("agz")

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

    def __int_value(self, key):
        if key not in self.data:
            # FIXME(knielsen): Hax, should be None, but is a "-" due to formatter in CLI
            return "-"
        return int(self.data[key])

    def __float_value(self, key):
        if key not in self.data:
            # FIXME(knielsen): Hax, should be None, but is a "-" due to formatter in CLI
            return "-"
        return float(self.data[key])

if __name__ == "__main__":
    logging.basicConfig(filename='tello.log', level=logging.DEBUG)

    tello_cmd = TelloCommand()
    tello_state = TelloState()
    controller = PS4Controller(0, tello_cmd)

    try:
        tello_cmd.connect()
        tello_state.listen()
        controller.listen()

        ui = TelloCliUi(tello_state)
        ui.take_control()
    finally:
        controller.stop()
        tello_state.close()
