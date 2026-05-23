ALPHABET = "ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠḡḢḣḤĥȦȧШḩЪЫЬЭĮį0123456789ĺĻļĽľĿŀABCDEFGHIJKLMNOPQRSTUVWXYZṛќѝŞşŠabcdefghijklmnopqrstuvwxyzŻżṽžſẀẁẂẃẄẅẆẇẈẉΊẋẌẍΎƏҐґƒẓΔƕƖẗẘẙҚқƜƝΞƟƠơҢңƤƥΦƧƨΩΪΫάέήίưᾱβγδεζҷᾸικλμẽξοπӁӂÃτÅÆÇψωϊϋỌύώϏÐǑǒǓÔϕӖϗῘÙῚӛӜӝÞӟàῡǢǣӤåæçǨῩӪӫìíӮӯðñỲỳôỵǶỷӸùῺΏǼǽþǿ"

def encode(data: bytes) -> str:
    return "".join(ALPHABET[b] for b in data)

def decode(text: str) -> bytes:
    return bytes(ord(ch) % 256 for ch in text)


if __name__ == "__main__":
    import os
    data = os.urandom(64)
    encoded = encode(data)
    decoded = decode(encoded)
    print(f"Original:  {data.hex()}")
    print(f"Encoded:   {encoded}")
    print(f"Decoded:   {decoded.hex()}")
    print(f"Round-trip: {'OK' if decoded == data else 'FAIL'}")
