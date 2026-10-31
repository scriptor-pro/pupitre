# SPDX-License-Identifier: GPL-3.0-or-later
"""Synthèse vocale muette pour Orca : rien n'est prononcé ni envoyé à speech-dispatcher.

Orca écrit quand même dans son journal de débogage chaque phrase qu'il aurait dite
(« SPEECH OUTPUT: '…' »), ce que lit scenario.py.
"""
from orca import speechserver


class SpeechServer(speechserver.SpeechServer):
    _instance = None

    @staticmethod
    def getFactoryName():
        return "Muet"

    @staticmethod
    def getSpeechServers():
        return [SpeechServer.get_speech_server()]

    @staticmethod
    def get_speech_server(info=None):
        if SpeechServer._instance is None:
            SpeechServer._instance = SpeechServer()
        return SpeechServer._instance

    @staticmethod
    def shutdownActiveServers():
        pass

    def get_info(self):
        return ["Muet", "muet"]

    def getVoiceFamilies(self):
        return []

    def list_output_modules(self):
        return []

    def getOutputModule(self):
        return ""
