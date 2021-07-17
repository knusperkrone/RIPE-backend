#! /usr/bin/python3

import os
import subprocess
import sys
import threading
import time

DEFAULT_TARGET = 'debug'
RELOAD_TARGET = 'hot-reload'


def start_ripe(lock):
    subprocess.Popen(['rm', f'target/{DEFAULT_TARGET}/*.so*'],
                     stderr=subprocess.PIPE).wait()
    subprocess.Popen(['rm', f'target/{RELOAD_TARGET}/*.so*'],
                     stderr=subprocess.PIPE).wait()
    subprocess.Popen(['rm', '-rf', f'target/{RELOAD_TARGET}/new'],
                     stderr=subprocess.PIPE).wait()
    os.system(
        f'cargo +nightly build --all -Z unstable-options --out-dir target/{RELOAD_TARGET}')
    lock.release()

    os.system(f'mkdir -p ./target/{RELOAD_TARGET}')
    os.environ.setdefault('PLUGIN_DIR', f'./target/{RELOAD_TARGET}')
    os.system('cargo +nightly run')
    print('RIPE crashed')
    exit(1)


def reload_plugins():
    print(f'Compiling..')

    build = subprocess.Popen(
        ['cargo', '+nightly', 'build', '--all', '--out-dir', f'target/{RELOAD_TARGET}/new', '-Z', 'unstable-options'], stderr=subprocess.PIPE)
    stdout, stderr = build.communicate()
    output = stderr.decode('utf-8')

    # extract compiled plugins from build command
    updated_plugins = []
    for line in output.splitlines():
        needle = 'Compiling '
        index = line.find(needle)
        if index != -1:
            name_index = index + len(needle)
            plugin_name = line[name_index: line.find(' ', name_index)]
            lib_name = f'lib{plugin_name}'
            updated_plugins.append(lib_name)

    # list, filter and count existing plugins
    base_dir = os.getcwd()
    libs = {}
    os.chdir(f'target/{RELOAD_TARGET}')

    for entry in os.listdir('.'):
        needle = '.so'
        index = entry.find(needle)
        if index != -1:
            key = entry[0:index]
            if key in updated_plugins:
                if not key in libs:
                    libs[key] = 0
                libs[key] += 1

    os.chdir('new')
    for k in libs:
        orig_name = f'{k}.so'
        new_name = f'{k}.so{libs[k] + 1}'
        subprocess.Popen(
            ['cp', orig_name, f'{base_dir}/target/{RELOAD_TARGET}/{new_name}'])
        print(f'updated {k}_v{libs[k] + 1}')
    os.chdir(base_dir)


def main():
    lock = threading.Semaphore(0)
    threading.Thread(target=start_ripe, name="Ripe executor",
                     args=(lock,)).start()
    lock.acquire()  # wait for app running
    time.sleep(1)

    while True:
        print('Press enter to reload')
        i = sys.stdin.read(1)
        reload_plugins()


if __name__ == '__main__':
    main()
